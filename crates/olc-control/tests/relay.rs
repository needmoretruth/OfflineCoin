//! Boards in a line, each hearing only its neighbours: the computer talks to the first and the
//! last is out of its range.
use olc_control::{
    sign, Action, BoardId, Body, Forward, Gate, Order, Packet, Read, Refusal, Router, Signed,
};
use std::collections::VecDeque;

const SECRET: [u8; 32] = [7; 32];

fn id(index: usize) -> BoardId {
    [index as u8 + 1; 6]
}

struct Line {
    routers: Vec<Router>,
    gates: Vec<Gate>,
    /// (from which node, to which node, packet)
    air: VecDeque<(usize, usize, Packet)>,
    /// Orders each node took, and the refusals.
    obeyed: Vec<Vec<Action>>,
    refused: Vec<Vec<Refusal>>,
    heard: Vec<Vec<Packet>>,
    /// Every time a node passed a packet on.
    relayed: usize,
    /// A relay in the middle that changes what it carries.
    bad_relay: Option<usize>,
}

impl Line {
    fn new(nodes: usize) -> Self {
        let key = olc_crypto::public_key(&SECRET);
        Self {
            routers: (0..nodes).map(|node| Router::new(id(node), 0)).collect(),
            gates: (0..nodes).map(|node| Gate::new(id(node), key, 0)).collect(),
            air: VecDeque::new(),
            obeyed: vec![Vec::new(); nodes],
            refused: vec![Vec::new(); nodes],
            heard: vec![Vec::new(); nodes],
            relayed: 0,
            bad_relay: None,
        }
    }

    fn neighbours(&self, node: usize) -> Vec<usize> {
        let last = self.routers.len() - 1;
        [node.checked_sub(1), (node < last).then_some(node + 1)]
            .into_iter()
            .flatten()
            .collect()
    }

    fn emit(&mut self, node: usize, forward: Forward, packet: &Packet) {
        let targets: Vec<usize> = match forward {
            Forward::Nowhere => Vec::new(),
            Forward::All => self.neighbours(node),
            Forward::Peer(peer) => vec![peer as usize],
            Forward::AllExcept(peer) => self
                .neighbours(node)
                .into_iter()
                .filter(|next| *next != peer as usize)
                .collect(),
        };
        for target in targets {
            self.air.push_back((node, target, packet.clone()));
        }
    }

    /// `node` starts a packet, the way the computer or a board does.
    fn start(&mut self, node: usize, to: Option<BoardId>, body: Body) {
        let forward = self.routers[node].outgoing(to);
        let packet = Packet {
            from: id(node),
            to,
            id: self.routers[node].next_id(),
            hops: 0,
            body,
        };
        self.emit(node, forward, &packet);
    }

    fn run(&mut self) {
        while let Some((from, node, mut packet)) = self.air.pop_front() {
            let verdict = self.routers[node].incoming(from as u32, &packet);
            if verdict.forward != Forward::Nowhere {
                self.relayed += 1;
                packet.hops += 1;
                let mut carried = packet.clone();
                if self.bad_relay == Some(node) {
                    change_order(&mut carried);
                }
                self.emit(node, verdict.forward, &carried);
            }
            if !verdict.for_me {
                continue;
            }
            self.heard[node].push(packet.clone());
            if let Body::Order(signed) = &packet.body {
                match self.gates[node].admit(signed) {
                    Ok(action) => self.obeyed[node].push(action),
                    Err(refusal) => self.refused[node].push(refusal),
                }
            }
        }
    }

    /// Every board says it is there, so the routes are learned as they are on real boards.
    fn beacons(&mut self) {
        for node in 0..self.routers.len() {
            self.start(node, None, Body::Read(Read::Status));
            self.run();
        }
    }
}

fn change_order(packet: &mut Packet) {
    if let Body::Order(signed) = &mut packet.body {
        signed.order.action = Action::Line("pay someone 1000".into());
    }
}

fn order(board: BoardId, counter: u64, line: &str) -> Signed {
    let action = Action::Line(line.into());
    let order = Order {
        board,
        counter,
        action,
    };
    sign(&SECRET, order).unwrap()
}

#[test]
fn a_signed_order_reaches_a_board_out_of_range_and_its_answer_comes_back() {
    let mut line = Line::new(3);
    line.beacons();
    let last = id(2);
    line.start(0, Some(last), Body::Order(order(last, 1, "limit 3000")));
    line.run();
    assert_eq!(line.obeyed[2], vec![Action::Line("limit 3000".into())]);
    assert!(line.obeyed[1].is_empty());

    line.start(2, Some(id(0)), Body::Line("done 1".into()));
    line.run();
    let answer = line.heard[0].last().unwrap();
    assert_eq!((answer.from, answer.hops), (last, 1));
}

#[test]
fn an_order_finds_its_way_before_any_route_is_known() {
    let mut line = Line::new(3);
    let last = id(2);
    line.start(0, Some(last), Body::Order(order(last, 1, "view easy")));
    line.run();
    assert_eq!(line.obeyed[2].len(), 1);
}

#[test]
fn a_relay_cannot_change_an_order() {
    let mut line = Line::new(3);
    line.beacons();
    line.bad_relay = Some(1);
    let last = id(2);
    line.start(0, Some(last), Body::Order(order(last, 1, "limit 3000")));
    line.run();
    assert!(line.obeyed[2].is_empty());
    assert_eq!(line.refused[2], vec![Refusal::BadSignature]);
}

#[test]
fn a_relay_cannot_write_an_order_of_its_own() {
    let mut line = Line::new(3);
    line.beacons();
    let last = id(2);
    let mut forged = order(last, 1, "pay someone 1000");
    forged.signature = sign(&[9; 32], forged.order.clone()).unwrap().signature;
    line.start(1, Some(last), Body::Order(forged));
    line.run();
    assert_eq!(line.refused[2], vec![Refusal::BadSignature]);
}

#[test]
fn a_copy_heard_twice_is_taken_once_and_not_passed_on() {
    let mut line = Line::new(3);
    line.beacons();
    let last = id(2);
    let packet = Packet {
        from: id(0),
        to: Some(last),
        id: 900,
        hops: 0,
        body: Body::Order(order(last, 1, "limit 3000")),
    };
    line.air.push_back((0, 1, packet.clone()));
    line.air.push_back((0, 1, packet));
    line.relayed = 0;
    line.run();
    assert_eq!(line.relayed, 1);
    assert_eq!(line.obeyed[2].len(), 1);
}

#[test]
fn a_packet_stops_at_the_hop_limit() {
    let mut line = Line::new(8);
    line.start(0, None, Body::Read(Read::Status));
    line.run();
    let reached: Vec<usize> = (0..8)
        .filter(|node| !line.heard[*node].is_empty())
        .collect();
    assert_eq!(reached, vec![1, 2, 3, 4, 5]);
}

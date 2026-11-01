// The experiment's panel on the app's page. It lives with the experiment, not with the coin.
Object.assign(words.en, {
  lab_title: "51% attack",
  lab_about: "A miner with most of the hash power can do this with ordinary software. It takes two boards about 15 minutes. Afterwards they return to their settings.",
  lab_attacker: "Attacker", lab_victim: "Victim", lab_start: "Start", lab_stop: "Stop",
});
Object.assign(words.ko, {
  lab_title: "51% 공격",
  lab_about: "해시 과반을 가진 채굴자라면 평범한 소프트웨어로 할 수 있는 일입니다. 보드 둘로 15분쯤 걸립니다. 끝나면 보드는 원래 설정으로 돌아갑니다.",
  lab_attacker: "공격자", lab_victim: "피해자", lab_start: "시작", lab_stop: "중지",
});
const labKorean = [
  [/^Attacker (\S+) at height (\d+)/, "공격자 $1, 높이 $2"],
  [/^Victim +(\S+) at height (\d+)/, "피해자 $1, 높이 $2"],
  [/^1\. Hash power/, "1. 해시 비율"],
  [/^ +attacker (\d+) H\/s, ([\d.]+)% of all hashing/, "   공격자 $1 H/s, 전체의 $2%"],
  [/^ +victim +(\d+) H\/s, ([\d.]+)% of all hashing/, "   피해자 $1 H/s, 전체의 $2%"],
  [/^ +Both boards agree at height (\d+)/, "   두 보드가 높이 $1에서 같습니다"],
  [/^2\. The attacker pays the victim (\S+) OLC/, "2. 공격자가 피해자에게 $1 OLC를 보냅니다"],
  [/^ +victim balance: (\S+) before, (\S+) confirmed, (\S+) now/, "   피해자 잔액: 처음 $1, 확인 때 $2, 지금 $3"],
  [/^ +victim balance (\S+) OLC/, "   피해자 잔액 $1 OLC"],
  [/^ +payment (\S+) reached the victim/, "   송금 $1 피해자에게 도착"],
  [/^3\. The attacker goes dark/, "3. 공격자가 네트워크를 끊습니다"],
  [/^4\. The victim mines the payment/, "4. 피해자가 송금을 채굴합니다"],
  [/^ +payment at height (\d+), (\d+) of (\d+) confirmations/, "   송금이 높이 $1에 들어갔습니다. 확인 $2/$3"],
  [/^ +payment confirmed, victim balance (\S+) OLC/, "   송금 확인 끝. 피해자 잔액 $1 OLC"],
  [/^5\. The attacker spends the same coins on itself, out of sight/, "5. 공격자가 같은 코인을 몰래 자기에게 보냅니다"],
  [/^ +private spend: at height (\d+)/, "   몰래 보낸 송금: 높이 $1"],
  [/^ +private spend (\S+)/, "   몰래 보낸 송금 $1"],
  [/^6\. The private chain against the public chain/, "6. 숨긴 체인과 공개 체인의 경쟁"],
  [/^ +attacker height (\d+) work (\d+) at (\d+) H\/s \| victim height (\d+) work (\d+) at (\d+) H\/s/,
    "   공격자 높이 $1 작업량 $2 $3 H/s | 피해자 높이 $4 작업량 $5 $6 H/s"],
  [/^ +the private chain is heavier/, "   숨긴 체인이 더 무거워졌습니다"],
  [/^7\. The attacker comes back/, "7. 공격자가 돌아옵니다"],
  [/^ +the victim dropped (\d+) blocks at height (\d+)/, "   피해자가 높이 $2에서 블록 $1개를 버렸습니다"],
  [/^8\. The payment goes back to the pool and is refused/, "8. 송금이 풀로 돌아갔다가 거부됩니다"],
  [/^ +the dropped blocks gave payment (\S+) back/, "   버려진 블록의 송금 $1 풀로 돌아옴"],
  [/^ +refused: its coins are already spent on the winning chain/, "   거부: 이 코인은 이긴 체인에서 이미 쓰였습니다"],
  [/^9\. Result/, "9. 결과"],
  [/^ +winning chain height (\d+)/, "   이긴 체인 높이 $1"],
  [/^ +payment: gone/, "   송금: 사라짐"],
  [/^Both boards are back to their cap and network state\./, "두 보드가 원래 상한과 네트워크 상태로 돌아왔습니다."],
  [/^Stopped: /, "중지: "],
];
const labLine = (line) => {
  if (language !== "ko") return line;
  const start = line.match(/^\n*/)[0];
  const body = line.slice(start.length);
  const rule = labKorean.find(([pattern]) => pattern.test(body));
  return start + (rule ? body.replace(rule[0], rule[1]) : body);
};
(() => {
  const attacker = make("select", { name: "attacker" });
  const victim = make("select", { name: "victim" });
  const start = worded("button", "lab_start", { type: "button" });
  const stop = worded("button", "lab_stop", { type: "button" });
  const log = make("pre", { hidden: true });
  const panel = make("div", { className: "panel" }, worded("b", "lab_title"), worded("p", "lab_about"),
    make("div", { className: "controls" }, control("lab_attacker", attacker), control("lab_victim", victim)),
    make("div", { className: "line" }, start, stop), log);
  $("extra-panels").append(panel);

  start.onclick = async () => {
    if (attacker.value === victim.value) return;
    await post("/api/extra/start", {
      command: "attack", boards: [attacker.value, victim.value], args: [],
    });
  };
  stop.onclick = () => post("/api/extra/stop", {});

  const offer = (select, boards) => {
    const held = select.value;
    select.replaceChildren(...boards.map((board) => make("option", {
      value: board.id, textContent: board.name || board.id,
    })));
    if (boards.some((board) => board.id === held)) select.value = held;
  };
  fleetListeners.push((state) => {
    const online = state.boards.filter((board) => board.online);
    offer(attacker, online);
    offer(victim, online);
    if (!victim.value || victim.value === attacker.value) {
      const other = online.find((board) => board.id !== attacker.value);
      if (other) victim.value = other.id;
    }
    start.disabled = state.run.running || online.length < 2;
    stop.disabled = !state.run.running;
    log.hidden = state.run.lines.length === 0;
    const text = state.run.lines.map(labLine).join("\n");
    if (log.textContent !== text) {
      log.textContent = text;
      log.scrollTop = log.scrollHeight;
    }
  });
})();

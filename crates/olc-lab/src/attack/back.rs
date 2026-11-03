//! The attacker's return: the hidden chain is shown, blocks are replaced, and the payment is
//! judged gone or still there.
use super::{short, Race, BRANCH_MOVE_WAIT};
use olc_board::Place;
use crate::steps::{Outcome, Step};

impl Race<'_> {
    /// The attacker shows its lighter chain anyway. Every node keeps the public chain, the
    /// attacker's hidden blocks are the ones replaced, and the payment stays.
    pub(super) fn come_back_behind(&mut self, payment: &str) -> Result<Outcome, String> {
        say!(self, "\n7. The attacker comes back behind");
        if self.helper.is_some() {
            self.victim.set_offline(false)?;
        } else {
            self.attacker.set_offline(false)?;
        }
        (self.plan.watch)(Step::Revealed);
        loop {
            let attacker = self.attacker.status()?;
            let victim = self.victim.status_within(BRANCH_MOVE_WAIT)?;
            if attacker.tip == victim.tip {
                break;
            }
            self.pause()?;
        }
        self.pause()?;
        let (height, replaced) = self.attacker.status()?.reorg;
        say!(self, "   the attacker dropped {replaced} blocks at height {height}");
        (self.plan.watch)(Step::Replaced { height, blocks: replaced, hidden_won: false });
        match self.victim.tx(payment)? {
            Place::Height(height) => {
                say!(self, "\n8. Result");
                say!(self, "   payment: still at height {height}");
                (self.plan.watch)(Step::Stays);
                Ok(Outcome::Stays)
            }
            _ => Err("the payment left the public chain.".into()),
        }
    }

    pub(super) fn come_back(&mut self) -> Result<(), String> {
        say!(self, "\n7. The attacker comes back");
        let earlier = self.victim.status()?.reorg;
        if self.helper.is_some() {
            self.victim.set_offline(false)?;
        } else {
            self.attacker.set_offline(false)?;
        }
        (self.plan.watch)(Step::Revealed);
        loop {
            let attacker = self.attacker.status()?;
            let victim = self.victim.status_within(BRANCH_MOVE_WAIT)?;
            if attacker.tip == victim.tip {
                // The board counts a branch move just after it makes it, so give it one more look.
                self.pause()?;
                let settled = self.victim.status()?;
                if settled.reorg == earlier {
                    return Err("the victim's own chain won the race.".into());
                }
                let (height, replaced) = settled.reorg;
                say!(
                    self,
                    "   the victim dropped {replaced} blocks at height {height}"
                );
                (self.plan.watch)(Step::Replaced { height, blocks: replaced, hidden_won: true });
                return Ok(());
            }
            self.pause()?;
        }
    }

    /// Bitcoin's rule: a dropped block's payments go back through the pool's check. This one fails
    /// only because the coins are gone, which is what makes the attack a double spend.
    pub(super) fn refused(&mut self, payment: &str) -> Result<(), String> {
        say!(
            self,
            "\n8. The payment goes back to the pool and is refused"
        );
        match self.victim.tx(payment)? {
            Place::Dropped(reason) if reason == "Some(MissingInput)" => {
                say!(
                    self,
                    "   the dropped blocks gave payment {} back",
                    short(payment)
                );
                say!(
                    self,
                    "   refused: its coins are already spent on the winning chain"
                );
                (self.plan.watch)(Step::Refused);
                Ok(())
            }
            Place::Dropped(other) => Err(format!("the payment was refused for {other}.")),
            _ => Err("the victim did not refuse the payment.".into()),
        }
    }

    pub(super) fn judge(
        &mut self,
        payment: &str,
        spend: &str,
        [before, confirmed]: [&str; 2],
    ) -> Result<(), String> {
        say!(self, "\n9. Result");
        let after = self.victim.status()?;
        let spent = match self.victim.tx(spend)? {
            Place::Height(height) => height,
            _ => return Err("the private spend is not on the winning chain.".into()),
        };
        let gone = |place| matches!(place, Place::Nowhere | Place::Dropped(_));
        if !gone(self.victim.tx(payment)?) || !gone(self.attacker.tx(payment)?) {
            return Err("the payment is still on the winning chain.".into());
        }
        say!(self, "   winning chain height {}", after.height);
        say!(self, "   payment: gone");
        say!(self, "   private spend: at height {spent}");
        say!(
            self,
            "   victim balance: {before} before, {confirmed} confirmed, {} now",
            after.balance
        );
        Ok(())
    }
}

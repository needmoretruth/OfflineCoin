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
  let lines = 0;
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
    if (state.run.lines.length !== lines) {
      lines = state.run.lines.length;
      log.textContent = state.run.lines.join("\n");
      log.scrollTop = log.scrollHeight;
    }
  });
})();

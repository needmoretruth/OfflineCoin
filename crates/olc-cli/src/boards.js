// The boards on the app's page. On a plain node /api/boards does not exist, and nothing shows.
const rows = new Map();
const started = new Set();
const fleetListeners = [];

function make(tag, props = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}
const worded = (tag, key, props = {}) => {
  const node = make(tag, props);
  node.dataset.text = key;
  node.textContent = say(key);
  return node;
};
const hinted = (key, props = {}) => {
  const node = make("input", { autocomplete: "off", spellcheck: false, ...props });
  node.dataset.hint = key;
  node.placeholder = say(key);
  return node;
};
const control = (key, ...nodes) => {
  const box = make("div", { className: "control" }, worded("span", key));
  box.append(...nodes);
  return box;
};
const choice = (values) => make("select", {}, ...values.map(([value, key]) => {
  const option = worded("option", key, { value });
  return option;
}));

// Runs a line on the board and shows what came of it.
async function run(row, button, line, doneKey) {
  button.disabled = true;
  const reply = await post("/api/board", { id: row.id, line }).catch(() => ({ error: "no_answer" }));
  button.disabled = false;
  row.note.textContent = reply.error ? say(reply.error) : doneKey ? say(doneKey) : "";
  row.note.className = reply.error ? "note bad" : "note good";
  return !reply.error;
}

// The one knob: a cap in hashes per second. `apply` takes null for the default, 0 for full.
function capControl(half, apply) {
  const presets = [["relaxed", () => null], ["half", () => cap.half], ["full", () => 0]]
    .map(([key, value]) => {
      const button = worded("button", key, { type: "button" });
      button.onclick = () => apply(value());
      return button;
    });
  const field = hinted("hashes", { inputMode: "numeric", name: "cap" });
  const set = worded("button", "set", { type: "button" });
  set.onclick = () => apply(Number(field.value) || 0);
  const cap = {
    half, field,
    box: make("div", { className: "line cap" }, ...presets, field, set),
    show(limit, relaxed) {
      const [gentle, middle, full] = presets;
      gentle.classList.toggle("chosen", limit === relaxed);
      middle.classList.toggle("chosen", limit === cap.half);
      full.classList.toggle("chosen", limit === 0);
    },
  };
  return cap;
}

const BOARD_HALF = 7500;
const COMPUTER_RELAXED = 20000;
const computerCap = capControl(0, async (limit) => {
  await post("/api/limit", { limit });
  refresh();
});
$("computer-cap").append(control("cap", computerCap.box));

function showComputerCap(state) {
  computerCap.half = Math.round(state.full_rate / 2);
  computerCap.show(state.limit, COMPUTER_RELAXED);
}

function makeRow(board) {
  const row = { id: board.id, last: board };
  row.title = make("span", { className: "title" });
  row.where = make("span", { className: "where" });
  row.facts = make("span", { className: "facts" });
  const head = make("button", { className: "head", type: "button" }, row.title, row.where, row.facts);
  head.setAttribute("aria-expanded", "false");
  row.controls = make("div", { className: "controls", hidden: true });
  head.onclick = () => {
    row.controls.hidden = !row.controls.hidden;
    head.setAttribute("aria-expanded", String(!row.controls.hidden));
  };

  row.cap = capControl(BOARD_HALF, (limit) =>
    run(row, row.cap.field, `limit ${limit ?? "default"}`));
  row.mine = make("button", { type: "button" });
  row.mine.onclick = () => run(row, row.mine, `mine ${row.last.mining ? "off" : "on"}`);
  row.offline = make("button", { type: "button" });
  row.offline.onclick = () => run(row, row.offline, `offline ${row.last.offline ? "off" : "on"}`);

  const to = hinted("to", { required: true, name: "to" });
  const amount = hinted("amount", { required: true, inputMode: "decimal", name: "amount" });
  const send = worded("button", "send", { className: "primary" });
  const pay = make("form", { className: "pay" }, to, amount, send);
  pay.onsubmit = async (event) => {
    event.preventDefault();
    if (await run(row, send, `pay ${to.value.trim()} ${amount.value.trim()}`, "sent")) pay.reset();
  };

  row.view = choice([["basic", "view_basic"], ["detail", "view_detail"], ["easy", "view_easy"]]);
  row.view.onchange = () => run(row, row.view, `view ${row.view.value}`);
  row.language = choice([["en", "language_en"], ["ko", "language_ko"]]);
  row.language.onchange = () => run(row, row.language, `lang ${row.language.value}`);
  const name = hinted("board_name", { name: "board-name", maxLength: 32 });
  const nameButton = worded("button", "save", { type: "button" });
  nameButton.onclick = async () => {
    if (name.value.trim() && await run(row, nameButton, `name ${name.value.trim()}`)) name.value = "";
  };
  row.build = make("span");
  const update = worded("button", "update", { type: "button" });
  update.onclick = async () => {
    update.disabled = true;
    await post("/api/update", { id: row.id });
    update.disabled = false;
  };

  row.note = make("p", { className: "note" });
  row.controls.append(
    control("cap", row.cap.box),
    control("state_offline", make("div", { className: "line" }, row.offline, row.mine)),
    make("div", { className: "wide" }, control("pay", pay)),
    control("screen", row.view),
    control("screen_language", row.language),
    control("board_name", make("div", { className: "line" }, name, nameButton)),
    control("firmware", make("div", { className: "line" }, row.build, update)),
    make("div", { className: "wide" }, row.note),
  );
  row.node = make("article", { className: "board" }, head, row.controls);
  return row;
}

function where(board) {
  if (!board.online) return say("silent");
  if (board.kind === "radio") {
    return board.through ? say("radio_through").replace("{b}", board.through) : say("radio");
  }
  return say(board.kind);
}

function fill(row, board) {
  row.last = board;
  row.node.classList.toggle("silent", !board.online);
  row.title.textContent = board.name || board.id.slice(-4).toUpperCase();
  row.where.textContent = board.job ? say(board.job) : where(board);
  row.cap.show(board.limit, board.default_cap);
  const facts = [
    [say("block"), board.height.toLocaleString()],
    [say("balance"), board.balance.toLocaleString(undefined, { maximumFractionDigits: 2 })],
    [say("hashes"), board.hashrate.toLocaleString()],
    [say("cap"), board.limit ? board.limit.toLocaleString() : say("full")],
    [say("peers"), board.peers],
  ];
  if (board.offline) facts.push([say("state_offline"), ""]);
  if (board.refused) facts.push([say("refused_count").replace("{n}", board.refused), ""]);
  row.facts.replaceChildren(...facts.map(([label, value]) => {
    const fact = make("span", {}, `${label} `);
    if (value !== "") fact.append(make("b", { textContent: value }));
    return fact;
  }));
  row.mine.textContent = say(board.mining ? "go_idle" : "go_mine");
  row.offline.textContent = say(board.offline ? "go_online" : "go_offline");
  if (document.activeElement !== row.view) row.view.value = board.view;
  if (document.activeElement !== row.language) row.language.value = board.language;
  row.build.textContent = board.build;
}

function drawPorts(ports) {
  const list = $("new-boards");
  const kept = new Map([...list.children].map((node) => [node.dataset.path, node]));
  list.replaceChildren(...ports.map((port) => {
    const box = kept.get(port.path) ?? makePort(port);
    box.status.textContent = port.job ? say(port.job) : "";
    box.install.disabled = port.job === "installing";
    return box;
  }));
}

function makePort(port) {
  const name = hinted("board_name", { required: true, maxLength: 32, name: "new-name" });
  const install = worded("button", "install", { className: "primary" });
  const form = make("form", { className: "line" }, name, install);
  const status = make("p", { className: "note" });
  const box = make("article", { className: "board new-board" }, worded("b", "new_board"),
    make("span", { className: "port", textContent: port.path.split("/").pop() }), form, status);
  box.dataset.path = port.path;
  box.status = status;
  box.install = install;
  form.onsubmit = async (event) => {
    event.preventDefault();
    install.disabled = true;
    await post("/api/install", { path: port.path, name: name.value.trim() });
  };
  return box;
}

function drawFleet(state) {
  $("boards").hidden = false;
  for (const board of state.boards) {
    if (!rows.has(board.id)) {
      rows.set(board.id, makeRow(board));
      $("board-list").append(rows.get(board.id).node);
    }
    fill(rows.get(board.id), board);
  }
  drawPorts(state.ports);
  for (const command of state.extras) {
    if (started.has(command)) continue;
    started.add(command);
    $("extras").hidden = false;
    document.body.append(make("script", { src: `/extra/${command}.js` }));
  }
  fleetListeners.forEach((listen) => listen(state));
}

let fleet = null;
async function refreshFleet() {
  const reply = await fetch("/api/boards").catch(() => null);
  if (!reply?.ok) return false;
  fleet = await reply.json().catch(() => null);
  if (fleet) drawFleet(fleet);
  return true;
}
document.addEventListener("words", () => { if (fleet) drawFleet(fleet); });
refreshFleet().then((app) => { if (app) setInterval(refreshFleet, 1000); });

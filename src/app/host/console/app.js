// toss-connector 웹 콘솔 스크립트. 별도 프레임워크나 번들러 없이 이 파일 하나로 동작한다.
//
// 실행 중인 toss-connector API(src/app/host/src/console.rs 참고)를 호출한다. 1.5초마다 /api/state를 폴링해
// 변경된 부분만 다시 렌더링하며, 응답이 늦어 순서가 어긋나면 이전 요청의 응답은 무시한다.
// API 주소와 토큰:
//   - toss-connector가 직접 서빙하는 페이지(http://127.0.0.1:<포트>/)라면 현재 origin을 그대로 사용한다.
//     토큰은 `toss-connector console` 실행 시 URL 해시(#token=...)로 전달되며, 읽자마자 주소창에서 제거하고 localStorage에 저장한다.
//   - 외부 정적 호스팅에 올려 둔 경우 "연결" 화면에서 주소(기본값 http://127.0.0.1:7880)와 토큰을 입력한다.
//     PC에서 해당 출처를 미리 허용해야 한다.
//   - 보안상 로컬 루프백 주소(http://127.0.0.1:<포트> 또는 http://localhost:<포트>)만 허용한다.
//     toss-connector는 그 주소에서만 리스닝하고, 토큰은 그 주소로만 전송한다.
//     URL 해시(#...)에서는 토큰만 읽고 API 주소는 받지 않으며, 연결 화면에서도 다른 호스트는 입력할 수 없다.
// NixOS 모듈(programs.toss-connector.config)로 관리되는 설정이 있으면 state.managed에 포함된다.
// 여기서 고정된 권한(perms.fixed)과 명령(commands.managed)은 UI에서 수정할 수 없도록 잠그고 "Nix" 태그를 표시한다(API에서도 거부됨).
// 사용자 입력(명령 이름, 커맨드 등)을 DOM에 삽입할 때는 XSS 방지를 위해 textContent만 사용한다.
// 외부 연동(Discord) 키 입력 칸은 HTML에 고정해 두어 1.5초마다 다시 그려도 입력 중인 값이 사라지지 않는다.
// Client Secret은 덱으로 보내기만 하고 저장에 성공하면 칸을 비운다(브라우저 저장소에도 남기지 않는다).
// Spotify 연결은 빈 탭을 먼저 연 뒤(팝업 차단을 피하려고 클릭 직후에 연다) API가 돌려준 Spotify 승인 주소로 보낸다.
// 로그인을 마치면 Spotify가 이 콘솔의 `/spotify/callback`으로 돌아오고, 그 탭에 결과가 표시된다.
// 명령 편집의 "Discord" 종류는 셸 커맨드 대신 Discord 동작(state.discord_actions)을 고른다. 실제 종류(스위치 등)는 동작이 정한다.
"use strict";

const $ = (id) => document.getElementById(id);
const local = location.protocol === "http:" && (location.hostname === "127.0.0.1" || location.hostname === "localhost");
const store = {
  get: (k) => { try { return localStorage.getItem("toss-connector." + k) || ""; } catch { return ""; } },
  set: (k, v) => { try { localStorage.setItem("toss-connector." + k, v); } catch { } },
};

const loopback = (url) => /^http:\/\/(127\.0\.0\.1|localhost):\d{1,5}$/.test(url);

const given = new URLSearchParams(location.hash.slice(1));
if (given.get("token")) store.set("token", given.get("token"));
if (location.hash) history.replaceState(null, "", location.pathname + location.search);

let api = loopback(store.get("api")) ? store.get("api") : local && loopback(location.origin) ? location.origin : "http://127.0.0.1:7880";
let token = store.get("token");
let state = null;
const drawn = {};
let failing = "";
let asked = 0;
let answered = 0;

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, "");
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) if (kid != null && kid !== false) el.append(kid);
  return el;
}

let toastTimer = 0;
function toast(text, bad) {
  const t = $("toast");
  t.textContent = text;
  t.className = bad ? "bad" : "";
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), bad ? 6000 : 2500);
}

async function call(method, path, body) {
  const res = await fetch(api + path, {
    method,
    headers: Object.assign({ Authorization: "Bearer " + token }, body ? { "Content-Type": "application/json" } : {}),
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw Object.assign(new Error(data.error || "요청에 실패했습니다 (" + res.status + ")"), { status: res.status });
  return data;
}

async function act(method, path, body, okText) {
  try {
    const data = await call(method, path, body);
    if (okText) toast(typeof okText === "function" ? okText(data) : okText);
    refresh();
    return data;
  } catch (e) {
    toast(e.message, true);
    return null;
  }
}

function ask(title, text, yes) {
  $("confirm-title").textContent = title;
  $("confirm-text").textContent = text;
  $("confirm-yes").textContent = yes;
  const d = $("confirm");
  return new Promise((done) => {
    d.onclose = () => done(d.returnValue === "yes");
    d.returnValue = "no";
    d.showModal();
  });
}

function showConnect(why) {
  failing = why;
  if ($("connect").hidden) $("api").value = api;
  $("connect").hidden = false;
  $("connect-why").textContent = why;
  $("origin-hint").textContent = api === location.origin ? "" : " 이 페이지는 toss-connector가 직접 제공한 페이지가 아닙니다. PC에서 출처를 먼저 허용하세요: toss-connector console allow " + location.origin;
  for (const id of ["decks-section", "commands-section", "links-section", "about-section"]) $(id).hidden = true;
  $("link").firstElementChild.className = "lamp bad";
  $("link-text").textContent = "연결 안 됨";
}

$("connect-go").onclick = () => {
  const next = $("api").value.trim().replace(/\/+$/, "") || api;
  if (!loopback(next)) return toast("로컬 주소만 입력할 수 있습니다: http://127.0.0.1:<포트> 또는 http://localhost:<포트>", true);
  token = $("token").value.trim() || token;
  api = next;
  $("origin-hint").textContent = "";
  store.set("token", token);
  store.set("api", api === location.origin ? "" : api);
  refresh();
};

async function refresh() {
  if (!token) return showConnect("토큰이 필요합니다.");
  const mine = ++asked;
  let next;
  try {
    next = await call("GET", "/api/state");
  } catch (e) {
    if (mine < answered) return;
    answered = mine;
    return showConnect(e.status === 401 ? "토큰이 올바르지 않습니다." : e.status === 403 ? e.message : "toss-connector에 연결할 수 없습니다 (" + api + "). 실행 중인지 확인하세요."
      + (api === location.origin ? "" : " 실행 중이라면 PC에서 이 페이지의 출처가 허용되지 않았을 수 있습니다."));
  }
  if (mine < answered) return;
  answered = mine;
  state = next;
  if (failing) {
    failing = "";
    $("connect").hidden = true;
    $("token").value = "";
  }
  for (const id of ["decks-section", "commands-section", "links-section", "about-section"]) $(id).hidden = false;
  draw();
}

function once(name, value, paint) {
  const text = JSON.stringify(value);
  if (drawn[name] === text) return;
  drawn[name] = text;
  paint();
}

function draw() {
  $("host").textContent = state.host.name + " · " + state.host.os;
  const up = state.decks.filter((d) => d.connected).length;
  $("link").firstElementChild.className = "lamp" + (up ? " on" : "");
  $("link-text").textContent = up ? "덱 연결됨" : state.decks.length ? "덱 연결 안 됨" : "페어링된 덱 없음";
  once("usb", state.usb_addr, () => ($("pair-addr").placeholder = "덱 주소 (USB: " + state.usb_addr + ")"));
  once("decks", [state.decks, state.permissions], drawDecks);
  once("pairing", state.pairing, drawPairing);
  once("commands", [state.commands, state.managed], drawCommands);
  once("links", [state.vault, state.discord, state.spotify, state.spotify_callback, state.decks.map((d) => [d.key, d.name])], drawLinks);
  once("about", [state.origins, state.host, state.managed, api], drawAbout);
}

function redrawDecks() {
  delete drawn.decks;
  draw();
}

function permRow(d, p) {
  const now = d.perms.find((x) => x.id === p.id) || { on: p.default, chosen: false, fixed: false };
  return h("div", { class: "row" },
    h("div", { class: "text" },
      h("div", { class: "title" }, p.title, now.fixed && h("span", { class: "tag nix", title: "시스템 설정(Nix)으로 고정되어 있어 여기서 변경할 수 없습니다" }, "Nix"),
        h("small", {}, p.id + (now.fixed || now.chosen ? "" : " · 기본값"))),
      h("div", { class: "note" }, p.about)),
    h("input", { type: "checkbox", class: "toggle", checked: now.on, disabled: now.fixed, "aria-label": p.title,
      onchange: async (e) => {
        const on = e.target.checked;
        if (!await act("POST", "/api/decks/" + d.key + "/perms", { id: p.id, on }, p.title + (on ? ": 허용됨" : ": 허용 안 함"))) redrawDecks();
      } }));
}

function deckBox(d) {
  return h("div", { class: "box" },
    h("div", { class: "row" },
      h("div", { class: "text" },
        h("div", { class: "title" }, d.name, h("span", { class: "tag" + (d.connected ? " on" : "") }, d.connected ? "연결됨 · " + d.addr : "연결 안 됨")),
        h("div", { class: "note mono" }, d.key.slice(0, 12) + "  ·  " + (d.addrs.join(", ") || "알려진 주소 없음"))),
      h("div", { class: "keys" },
        h("button", { class: "key", disabled: !d.connected, title: "덱이 Wi-Fi에 연결되어 있을 때만 사용할 수 있습니다",
          onclick: () => act("POST", "/api/decks/" + d.key + "/sleep", null, (r) => r.name + "이(가) 절전 모드로 전환됩니다") }, "절전"),
        h("button", { class: "key", disabled: d.connected || !d.can_wake, title: d.can_wake ? "덱의 Wi-Fi로 매직 패킷을 전송합니다" : "깨울 Wi-Fi MAC 주소를 아직 모릅니다",
          onclick: async (e) => {
            e.target.disabled = true;
            toast("매직 패킷을 전송했습니다. 깨어나는지 확인 중... (최대 20초)");
            await act("POST", "/api/decks/" + d.key + "/wake", null, (r) => "깨어났습니다 (" + r.addr + ")");
            redrawDecks();
          } }, "깨우기"),
        h("button", { class: "key",
          onclick: async () => {
            if (await ask("페어링 해제", d.name + "의 페어링을 이 PC에서 삭제합니다. 덱 설정에서도 이 PC를 삭제해야 다시 페어링할 수 있습니다.", "삭제"))
              act("POST", "/api/decks/" + d.key + "/forget", null, "삭제되었습니다");
          } }, "삭제"))),
    state.permissions.map((p) => permRow(d, p)));
}

function drawDecks() {
  $("decks").replaceChildren(...state.decks.map(deckBox));
}

const pairText = {
  connecting: "덱에 연결 중...",
  code: "덱 화면에도 같은 숫자가 표시되는지 확인하세요. 다르면 보안 위협일 수 있으니 취소하세요.",
  waiting: "덱에서 ACCEPT를 누를 때까지 기다리는 중...",
  done: "페어링되었습니다.",
};

function drawPairing() {
  const p = state.pairing, box = $("pair-progress");
  const busy = ["connecting", "code", "waiting"].includes(p.phase);
  $("pair-go").disabled = busy;
  box.hidden = p.phase === "idle";
  box.replaceChildren();
  if (p.phase === "idle") return;
  if (p.name) box.append(h("p", {}, "덱: ", h("b", {}, p.name), "  (" + p.addr + ")"));
  if (p.phase === "code" || p.phase === "waiting") box.append(h("div", { class: "code" }, p.code));
  box.append(h("p", { class: p.phase === "failed" ? "error" : "note" }, p.phase === "failed" || (p.phase === "done" && p.error) ? p.error : pairText[p.phase] || ""));
  const keys = h("div", { class: "keys" });
  if (p.phase === "code") keys.append(h("button", { class: "key accent", onclick: () => act("POST", "/api/pair/confirm") }, "숫자 일치"));
  keys.append(h("button", { class: "key", onclick: () => act("POST", "/api/pair/cancel") }, busy ? "취소" : "닫기"));
  box.append(keys);
}

$("pair-go").onclick = () => act("POST", "/api/pair", { addr: $("pair-addr").value.trim() });

const kindName = { run: "실행", switch: "스위치", stepped: "단계", linear: "범위" };

function commandLines(c, kind) {
  const lines = [];
  const line = (what, text) => lines.push(h("dt", {}, what), h("dd", { class: "mono" }, text));
  if (c.discord) {
    const a = discordAction(c);
    line("Discord", c.discord + (a ? " — " + a.title : ""));
    return lines;
  }
  if (kind === "run") line("커맨드", c.run);
  if (kind === "switch") { line("켜기", c.on); line("끄기", c.off); }
  if (kind === "stepped") {
    for (const s of c.steps) line("단계 " + s.name, s.run || "(공통 커맨드)");
    if (c.set) line("공통", c.set);
  }
  if (kind === "linear") { line("범위", (c.min || 0) + " ~ " + (c.max || 0) + " (단위 " + (c.step || 1) + ")"); line("설정", c.set); }
  if (c.state) line("상태", c.state + (c.every ? "   (" + c.every + "초마다)" : ""));
  return lines;
}

function discordAction(c) {
  return c.discord ? (state.discord_actions || []).find((a) => a.id === c.discord) : null;
}

function commandTryIt(c, kind) {
  const tryIt = h("div", { class: "keys" });
  const shown = h("span", { class: "value" });
  const set = (value) => act("POST", "/api/commands/" + c.id + "/set", { value }, (r) => c.name + " = " + r.text);
  if (kind === "run") tryIt.append(h("button", { class: "key", onclick: () => act("POST", "/api/commands/" + c.id + "/run", null, "실행됨: " + c.name) }, "테스트 실행"));
  if (kind === "switch") tryIt.append(h("button", { class: "key", onclick: () => set("on") }, "켜기"), h("button", { class: "key", onclick: () => set("off") }, "끄기"));
  if (kind === "stepped") {
    const pick = h("select", {}, c.steps.map((s) => h("option", {}, s.name)));
    tryIt.append(pick, h("button", { class: "key", onclick: () => set(pick.value) }, "적용"));
  }
  if (kind === "linear") {
    const num = h("input", { type: "number", step: c.step || 1, min: c.min || 0, max: c.max || 0, value: c.min || 0 });
    tryIt.append(num, h("button", { class: "key", onclick: () => set(Number(num.value)) }, "적용"));
  }
  if (c.state || c.discord) tryIt.append(h("button", { class: "key", onclick: async () => {
    const r = await act("GET", "/api/commands/" + c.id + "/state");
    if (r) shown.textContent = "현재: " + r.text;
  } }, "상태 조회"), shown);
  return tryIt;
}

function commandBox(c, strict) {
  const kind = c.kind || "run";
  return h("div", { class: "box" },
    h("div", { class: "row" },
      h("div", { class: "text" }, h("div", { class: "title" }, c.name, c.discord && h("span", { class: "tag" }, "Discord"), h("span", { class: "tag" }, kindName[kind] || kind),
        c.managed && h("span", { class: "tag nix", title: "시스템 설정(Nix)으로 고정된 명령입니다" }, "Nix"), h("small", { class: "mono" }, c.id))),
      !c.managed && !strict && h("div", { class: "keys" },
        h("button", { class: "key", onclick: () => openEditor(c) }, "편집"),
        h("button", { class: "key", onclick: async () => {
          if (await ask("명령 삭제", c.name + " 명령을 삭제합니다. 이 명령을 연결해 둔 덱 위젯은 비활성화되며, 같은 이름으로 다시 등록해도 기존 위젯과 다시 연결되지 않습니다.", "삭제"))
            act("DELETE", "/api/commands/" + c.id, null, "삭제됨: " + c.name);
        } }, "삭제"))),
    h("div", { class: "sub" }, h("dl", { class: "lines" }, commandLines(c, kind))),
    h("div", { class: "row" }, commandTryIt(c, kind)));
}

function drawCommands() {
  const strict = !!(state.managed && state.managed.strict);
  $("command-add").hidden = strict;
  $("commands-locked").hidden = !state.managed;
  $("commands-locked").textContent = !state.managed ? ""
    : strict ? "명령 목록이 시스템 설정(Nix)으로 잠겨 있어 여기서 추가하거나 수정할 수 없습니다."
    : "Nix 태그가 붙은 명령은 시스템 설정으로 고정되어 수정·삭제할 수 없습니다. 그 외의 명령은 자유롭게 추가할 수 있습니다.";
  $("commands").replaceChildren(...(state.commands.length
    ? state.commands.map((c) => commandBox(c, strict))
    : [h("div", { class: "box" }, h("div", { class: "empty" }, "등록된 명령이 없습니다."))]));
}

let editing = null;
const stateHint = {
  switch: "— 출력이 on/off이거나 종료 코드가 0이면 켜짐. 비우면 마지막 설정값만 기억합니다",
  stepped: "— 단계 이름을 출력. 비우면 마지막 설정값만 기억합니다",
  linear: "— 숫자를 출력. 비우면 마지막 설정값만 기억합니다",
};

function stepRow(name, run) {
  const row = h("div", { class: "step" },
    h("input", { type: "text", placeholder: "이름", maxlength: 16, value: name || "" }),
    h("input", { type: "text", class: "mono", spellcheck: "false", placeholder: "커맨드 (비우면 공통 커맨드 사용)", value: run || "" }),
    h("button", { type: "button", class: "key small", onclick: () => row.remove() }, "제거"));
  return row;
}

function showKind() {
  const kind = $("e-kind").value;
  for (const el of document.querySelectorAll("#editor [data-kind]")) el.hidden = !el.dataset.kind.split(" ").includes(kind);
  $("e-state-hint").textContent = stateHint[kind] || "";
}

function openEditor(c) {
  editing = c || null;
  $("editor-title").textContent = c ? "명령 편집" : "명령 추가";
  $("editor-save").textContent = c ? "저장" : "등록";
  $("editor-error").hidden = true;
  c = c || { kind: "run", steps: [{ name: "" }, { name: "" }] };
  $("e-name").value = c.name || "";
  $("e-discord").replaceChildren(...(state.discord_actions || []).map((a) => h("option", { value: a.id }, a.title + " (" + (kindName[a.kind] || a.kind) + ")")));
  $("e-kind").value = c.discord ? "discord" : c.kind || "run";
  if (c.discord) $("e-discord").value = c.discord;
  $("e-kind").disabled = !!editing;
  $("e-run").value = c.run || "";
  $("e-on").value = c.on || "";
  $("e-off").value = c.off || "";
  $("e-steps").replaceChildren(...(c.steps || []).map((s) => stepRow(s.name, s.run)));
  $("e-set-stepped").value = c.kind === "stepped" ? c.set || "" : "";
  $("e-set-linear").value = c.kind === "linear" ? c.set || "" : "";
  $("e-min").value = c.min || 0;
  $("e-max").value = c.kind === "linear" ? c.max || 0 : 100;
  $("e-step").value = c.step || 1;
  $("e-state").value = c.state || "";
  $("e-every").value = c.every || "";
  showKind();
  $("editor").showModal();
}

$("e-kind").onchange = showKind;
$("e-step-add").onclick = () => $("e-steps").append(stepRow());
$("command-add").onclick = () => openEditor(null);
$("editor-cancel").onclick = () => $("editor").close();

$("editor-form").onsubmit = async (e) => {
  e.preventDefault();
  const kind = $("e-kind").value;
  const spec = kind === "discord" ? { name: $("e-name").value, discord: $("e-discord").value } : { name: $("e-name").value, kind };
  if (kind === "run") spec.run = $("e-run").value;
  if (kind === "switch") Object.assign(spec, { on: $("e-on").value, off: $("e-off").value });
  if (kind === "stepped") {
    spec.steps = [...$("e-steps").children].map((r) => ({ name: r.children[0].value, run: r.children[1].value })).filter((s) => s.name.trim() || s.run.trim());
    spec.set = $("e-set-stepped").value;
  }
  if (kind === "linear") Object.assign(spec, { min: Number($("e-min").value), max: Number($("e-max").value), step: Number($("e-step").value), set: $("e-set-linear").value });
  if (kind !== "run" && kind !== "discord") {
    spec.state = $("e-state").value;
    if ($("e-every").value) spec.every = Number($("e-every").value);
  }
  try {
    await call(editing ? "PUT" : "POST", "/api/commands" + (editing ? "/" + editing.id : ""), spec);
    $("editor").close();
    toast(editing ? "저장되었습니다" : "등록되었습니다");
    refresh();
  } catch (err) {
    $("editor-error").textContent = err.message;
    $("editor-error").hidden = false;
  }
};

const discordSteps = {
  deck: "덱의 응답을 기다리는 중... 이 PC가 처음이면 덱 화면에서 ALLOW를 누르세요.",
  link: "이 PC의 Discord 앱에 뜬 승인 창에서 승인하세요...",
  check: "Discord 앱에 인증하는 중...",
};

function drawLinks() {
  const v = state.vault, job = state.discord;
  const s = v && v.services.find((x) => x.id === "discord");
  const busy = job.phase === "deck" || job.phase === "discord";
  const tag = (text, on) => h("span", { class: "tag" + (on ? " on" : "") }, text);
  $("discord-tags").replaceChildren(...(s ? [tag(s.registered ? "키 저장됨" : "키 없음", s.registered), tag(s.linked ? "연결됨" : "연결 안 됨", s.linked),
    tag(s.allowed ? "이 PC 허용됨" : "이 PC 허용 전", s.allowed)] : []));
  const deck = v && state.decks.find((d) => d.key === v.deck);
  $("discord-deck").textContent = !v ? "연결된 덱이 없거나 덱이 금고를 지원하지 않습니다."
    : "키와 토큰은 덱 " + (deck ? deck.name : v.deck.slice(0, 12)) + "에 보관됩니다.";
  $("discord-link").disabled = !s || !s.registered || busy;
  $("discord-check").disabled = !s || !s.linked || busy;
  const live = state.discord_live || {};
  $("discord-live").textContent = !live.held ? "Discord 동작 명령: 쓰는 덱이 없어 Discord 앱에 상주 연결하지 않았습니다."
    : live.up ? "Discord 동작 명령: Discord 앱에 연결되어 있습니다." : "Discord 동작 명령: 연결 대기 중" + (live.error ? " — " + live.error : "");
  $("discord-forget").disabled = !s || !s.registered || busy;
  $("d-save").disabled = !v || busy;
  const p = $("discord-progress");
  p.hidden = job.phase === "idle";
  p.replaceChildren();
  if (job.phase === "idle") return;
  const text = job.phase === "deck" ? discordSteps.deck : job.phase === "discord" ? discordSteps[job.kind]
    : job.phase === "done" ? "인증됨: " + job.who.name + (job.who.expires ? " · 토큰 만료 " + job.who.expires : "") : job.error;
  p.append(h("p", { class: job.phase === "failed" ? "error" : "note" }, text));
  if (!busy) p.append(h("div", { class: "keys" }, h("button", { class: "key small", onclick: () => act("POST", "/api/discord/clear") }, "닫기")));
  drawSpotify(v);
}

function drawSpotify(v) {
  const s = v && v.services.find((x) => x.id === "spotify");
  const job = state.spotify || { phase: "idle" };
  const busy = job.phase === "browser" || job.phase === "deck";
  const tag = (text, on) => h("span", { class: "tag" + (on ? " on" : "") }, text);
  $("spotify-tags").replaceChildren(...(s ? [tag(s.registered ? "키 저장됨" : "키 없음", s.registered), tag(s.linked ? "연결됨" : "연결 안 됨", s.linked)] : []));
  $("s-redirect").value = state.spotify_callback || "";
  $("spotify-link").disabled = !s || !s.registered || job.phase === "deck";
  $("spotify-forget").disabled = !s || !s.registered || busy;
  $("s-save").disabled = !s || busy;
  const p = $("spotify-progress");
  p.hidden = job.phase === "idle";
  p.replaceChildren();
  if (job.phase === "idle") return;
  const text = job.phase === "browser" ? "브라우저에서 Spotify에 로그인하고 승인하세요..."
    : job.phase === "deck" ? "덱이 Spotify 토큰을 발급받는 중..."
    : job.phase === "done" ? "Spotify 연결을 마쳤습니다. 덱이 재생 정보를 직접 가져옵니다." : job.error;
  p.append(h("p", { class: job.phase === "failed" ? "error" : "note" }, text));
  if (job.phase !== "deck") p.append(h("div", { class: "keys" }, h("button", { class: "key small", onclick: () => act("POST", "/api/spotify/clear") }, "닫기")));
}

$("discord-form").onsubmit = async (e) => {
  e.preventDefault();
  toast("덱에 저장하는 중... 이 PC가 처음이면 덱 화면에서 ALLOW를 누르세요.");
  const saved = await act("POST", "/api/vault/discord/put",
    { client_id: $("d-id").value, client_secret: $("d-secret").value, redirect_uri: $("d-redirect").value }, "덱에 저장했습니다. 이 PC에는 남기지 않았습니다.");
  if (saved) $("d-secret").value = "";
};
$("discord-link").onclick = () => act("POST", "/api/discord/link");
$("spotify-form").onsubmit = async (e) => {
  e.preventDefault();
  toast("덱에 저장하는 중... 이 PC가 처음이면 덱 화면에서 ALLOW를 누르세요.");
  const saved = await act("POST", "/api/vault/spotify/put",
    { client_id: $("s-id").value, client_secret: $("s-secret").value, redirect_uri: $("s-redirect").value }, "덱에 저장했습니다. 이 PC에는 남기지 않았습니다.");
  if (saved) $("s-secret").value = "";
};
$("spotify-link").onclick = async () => {
  const tab = window.open("about:blank", "_blank");
  const r = await act("POST", "/api/spotify/link");
  if (r && r.url && tab) tab.location = r.url;
  else if (tab) tab.close();
  if (r && r.url && !tab) toast("팝업이 막혔습니다. 이 주소를 직접 여세요: " + r.url, true);
};
$("spotify-forget").onclick = async () => {
  if (await ask("Spotify 키 삭제", "덱에 보관한 Spotify 앱 키와 토큰을 삭제합니다. 다시 쓰려면 키를 저장하고 다시 연결해야 합니다.", "삭제"))
    act("POST", "/api/vault/spotify/forget", null, "삭제되었습니다");
};
$("discord-check").onclick = () => act("POST", "/api/discord/check");
$("discord-forget").onclick = async () => {
  if (await ask("Discord 키 삭제", "덱에 보관한 Discord 앱 키와 토큰을 삭제합니다. 다시 쓰려면 키를 저장하고 다시 연결해야 합니다.", "삭제"))
    act("POST", "/api/vault/discord/forget", null, "삭제되었습니다");
};

function drawAbout() {
  const a = $("about");
  a.replaceChildren(
    h("p", {}, "toss-connector " + state.host.version + " · " + api),
    h("p", {}, api === location.origin ? "이 페이지는 로컬 toss-connector에서 직접 제공하고 있습니다." : "이 페이지는 " + location.origin + " 에서 열렸습니다 (허용된 출처)."),
    h("p", {}, !state.managed ? "시스템 관리 설정(Nix)이 없습니다. 모든 권한과 명령을 여기서 편집할 수 있습니다."
      : state.managed.strict ? "시스템 설정(Nix)이 strict 모드입니다. 설정에 정의된 권한과 명령만 사용됩니다."
      : "시스템 설정(Nix)으로 일부 권한과 명령이 고정되어 있습니다. 나머지 항목은 여기서 추가로 설정할 수 있습니다."),
    h("p", {}, state.origins.length ? "허용된 외부 출처: " + state.origins.join(", ") : "허용된 외부 출처가 없습니다."),
    h("p", {}, "출처 허용/해제는 터미널에서만 변경할 수 있습니다: ", h("code", {}, "toss-connector console allow <출처>"), " / ", h("code", {}, "deny <출처>")));
}

refresh();
setInterval(() => { if (!document.hidden) refresh(); }, 1500);

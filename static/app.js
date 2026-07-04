"use strict";

const POSITIONS = ["P", "C", "1B", "2B", "3B", "SS", "LF", "CF", "RF", "DH", "EH"];

const state = {
    user: null,
    teams: [],
    teamId: null,
    team: null,
    players: [],
    lineups: [],
    lineupId: null,
    // spots keyed by a stable slot id: "1".."9", "DH", "EH"
    assignments: {}, // slotId -> playerId
    positions: {}, // slotId -> position string
    twoWayDH: {}, // slotId -> bool (high-school two-way DH: fields AND is the DH)
    meta: { opponent: "", game_date: "", location: "", home_away: "", use_dh: false, use_eh: false, name: "" },
    dirty: false,
};

// ---------- API helper ----------
async function api(method, path, body, isForm) {
    const opts = { method, credentials: "same-origin", headers: {} };
    if (isForm) {
        opts.body = body;
    } else if (body !== undefined) {
        opts.headers["Content-Type"] = "application/json";
        opts.body = JSON.stringify(body);
    }
    const res = await fetch("/api" + path, opts);
    if (!res.ok) {
        let msg = res.statusText;
        try { msg = (await res.json()).error || msg; } catch (_) {}
        const err = new Error(msg);
        err.status = res.status;
        throw err;
    }
    const ct = res.headers.get("content-type") || "";
    return ct.includes("application/json") ? res.json() : res;
}

const $ = (id) => document.getElementById(id);
const el = (tag, cls, txt) => { const e = document.createElement(tag); if (cls) e.className = cls; if (txt != null) e.textContent = txt; return e; };

function setStatus(msg, isError) {
    const s = $("status-msg");
    s.textContent = msg;
    s.style.color = isError ? "var(--red)" : "var(--accent)";
    if (msg) setTimeout(() => { if (s.textContent === msg) s.textContent = ""; }, 4000);
}

// ---------- Auth ----------
let authMode = "login";

function showAuth() {
    $("auth-view").classList.remove("hidden");
    $("app-view").classList.add("hidden");
}
function showApp() {
    $("auth-view").classList.add("hidden");
    $("app-view").classList.remove("hidden");
}

function wireAuth() {
    const setMode = (m) => {
        authMode = m;
        $("tab-login").classList.toggle("active", m === "login");
        $("tab-signup").classList.toggle("active", m === "signup");
        $("auth-submit").textContent = m === "login" ? "Log in" : "Create account";
        $("auth-password").autocomplete = m === "login" ? "current-password" : "new-password";
        $("auth-error").textContent = "";
    };
    $("tab-login").onclick = () => setMode("login");
    $("tab-signup").onclick = () => setMode("signup");

    $("auth-form").onsubmit = async (e) => {
        e.preventDefault();
        $("auth-error").textContent = "";
        const email = $("auth-email").value.trim();
        const password = $("auth-password").value;
        try {
            const user = await api("POST", authMode === "login" ? "/login" : "/signup", { email, password });
            state.user = user;
            await bootApp();
        } catch (err) {
            $("auth-error").textContent = err.message;
        }
    };
}

// ---------- Boot ----------
async function bootApp() {
    $("user-email").textContent = state.user.email;
    showApp();
    await loadTeams();
}

async function loadTeams() {
    state.teams = await api("GET", "/teams");
    const sel = $("team-select");
    sel.innerHTML = "";
    for (const t of state.teams) {
        const o = el("option", null, t.name);
        o.value = t.id;
        sel.appendChild(o);
    }
    if (state.teams.length === 0) {
        const created = await api("POST", "/teams", { name: "My Team" });
        state.teams = [created];
        const o = el("option", null, created.name);
        o.value = created.id;
        sel.appendChild(o);
    }
    state.teamId = state.teams[0].id;
    sel.value = state.teamId;
    await selectTeam(state.teamId);
}

async function selectTeam(id) {
    state.teamId = id;
    state.team = state.teams.find((t) => t.id == id);
    fillTeamFields();
    state.players = await api("GET", `/teams/${id}/players`);
    state.lineups = await api("GET", `/teams/${id}/lineups`);
    fillLineupSelect();
    newLineup();
    renderRoster();
    renderAll();
}

function fillTeamFields() {
    $("team-name").value = state.team.name || "";
    $("team-head").value = state.team.head_coach || "";
    $("team-assts").value = state.team.assistant_coaches || "";
    const lp = $("logo-preview");
    if (state.team.logo_path) {
        lp.style.backgroundImage = `url(${state.team.logo_path})`;
        lp.textContent = "";
    } else {
        lp.style.backgroundImage = "";
        lp.textContent = "No logo";
    }
}

function fillLineupSelect() {
    const sel = $("lineup-select");
    sel.innerHTML = '<option value="">— new lineup —</option>';
    for (const l of state.lineups) {
        const label = (l.name && l.name.trim()) || `${l.opponent || "Game"} ${l.game_date || ""}`.trim();
        const o = el("option", null, label || `Lineup #${l.id}`);
        o.value = l.id;
        sel.appendChild(o);
    }
    sel.value = state.lineupId || "";
}

// ---------- Lineup state ----------
function slotIds() {
    const ids = [];
    for (let i = 1; i <= 9; i++) ids.push(String(i));
    if (state.meta.use_dh) ids.push("DH");
    if (state.meta.use_eh) ids.push("EH");
    return ids;
}

function newLineup() {
    state.lineupId = null;
    state.assignments = {};
    state.positions = {};
    state.twoWayDH = {};
    state.meta = { opponent: "", game_date: "", location: "", home_away: "", use_dh: false, use_eh: false, name: "" };
    // default fielding positions for the first nine spots
    for (let i = 1; i <= 9; i++) state.positions[String(i)] = POSITIONS[i - 1] || "";
    fillMetaFields();
    $("lineup-select").value = "";
}

function fillMetaFields() {
    $("meta-opponent").value = state.meta.opponent;
    $("meta-date").value = state.meta.game_date;
    $("meta-location").value = state.meta.location;
    $("meta-homeaway").value = state.meta.home_away;
    $("meta-dh").checked = state.meta.use_dh;
    $("meta-eh").checked = state.meta.use_eh;
    $("meta-name").value = state.meta.name;
}

async function loadLineup(id) {
    const detail = await api("GET", `/lineups/${id}`);
    state.lineupId = detail.id;
    state.meta = {
        opponent: detail.opponent || "",
        game_date: detail.game_date || "",
        location: detail.location || "",
        home_away: detail.home_away || "",
        use_dh: !!detail.use_dh,
        use_eh: !!detail.use_eh,
        name: detail.name || "",
    };
    state.assignments = {};
    state.positions = {};
    state.twoWayDH = {};
    for (const sp of detail.spots) {
        const slot = sp.slot_kind === "DH" ? "DH" : sp.slot_kind === "EH" ? "EH" : String(sp.batting_order);
        if (sp.player_id != null) state.assignments[slot] = sp.player_id;
        state.positions[slot] = sp.position || "";
        if (sp.is_dh) state.twoWayDH[slot] = true;
    }
    fillMetaFields();
    renderAll();
}

function collectSpots() {
    const spots = [];
    for (const id of slotIds()) {
        const kind = id === "DH" ? "DH" : id === "EH" ? "EH" : "BAT";
        const order = id === "DH" ? 10 : id === "EH" ? 11 : parseInt(id, 10);
        spots.push({
            batting_order: order,
            slot_kind: kind,
            player_id: state.assignments[id] != null ? state.assignments[id] : null,
            position: state.positions[id] || "",
            is_dh: !!state.twoWayDH[id],
        });
    }
    return spots;
}

function lineupPayload() {
    return {
        name: state.meta.name,
        opponent: state.meta.opponent,
        game_date: state.meta.game_date,
        location: state.meta.location,
        home_away: state.meta.home_away,
        use_dh: state.meta.use_dh,
        use_eh: state.meta.use_eh,
        spots: collectSpots(),
    };
}

// ---------- Assignment helpers ----------
function assign(slotId, playerId) {
    // Remove this player from any other slot.
    for (const k of Object.keys(state.assignments)) {
        if (state.assignments[k] == playerId) delete state.assignments[k];
    }
    state.assignments[slotId] = playerId;
    // Default the position to the player's default if the slot has none.
    if (!state.positions[slotId]) {
        const p = state.players.find((pl) => pl.id == playerId);
        if (p && p.default_position) state.positions[slotId] = p.default_position;
    }
    renderAll();
}

function unassign(slotId) {
    delete state.assignments[slotId];
    renderAll();
}

function placedPlayerIds() {
    return new Set(Object.values(state.assignments).map(Number));
}

// ---------- Rendering ----------
function renderAll() {
    renderSpots();
    renderPool();
    renderPreview();
}

function renderRoster() {
    const list = $("roster-list");
    list.innerHTML = "";
    if (state.players.length === 0) {
        list.appendChild(el("li", "hint", "No players yet — add some above."));
        return;
    }
    for (const p of state.players) {
        const li = el("li");
        li.appendChild(el("span", "rnum", p.number || "—"));
        li.appendChild(el("span", "rname", p.name));
        li.appendChild(el("span", "rpos", p.default_position || ""));
        const edit = el("button", "icon-btn", "✎");
        edit.title = "Edit"; edit.onclick = () => editPlayer(p);
        const del = el("button", "icon-btn", "✕");
        del.title = "Delete"; del.onclick = () => deletePlayer(p);
        li.appendChild(edit);
        li.appendChild(del);
        list.appendChild(li);
    }
}

function renderSpots() {
    const ol = $("lineup-spots");
    ol.innerHTML = "";
    for (const id of slotIds()) {
        const isSpecial = id === "DH" || id === "EH";
        const li = el("li", "spot" + (id === "DH" ? " dh" : id === "EH" ? " eh" : ""));

        const order = el("span", "order", isSpecial ? id : id);
        li.appendChild(order);

        const slot = el("div", "slot");
        slot.dataset.slot = id;
        const pid = state.assignments[id];
        if (pid != null) {
            const p = state.players.find((pl) => pl.id == pid);
            slot.classList.add("filled");
            slot.draggable = true;
            slot.appendChild(el("span", "snum", p ? (p.number || "—") : "?"));
            slot.appendChild(el("span", "sname", p ? p.name : "(removed player)"));
            slot.addEventListener("dragstart", (e) => {
                e.dataTransfer.setData("text/plain", String(pid));
                e.dataTransfer.effectAllowed = "move";
            });
        } else {
            slot.appendChild(el("span", "placeholder", "Drop player here"));
        }
        slot.addEventListener("dragover", (e) => { e.preventDefault(); slot.classList.add("dragover"); });
        slot.addEventListener("dragleave", () => slot.classList.remove("dragover"));
        slot.addEventListener("drop", (e) => {
            e.preventDefault();
            slot.classList.remove("dragover");
            const playerId = parseInt(e.dataTransfer.getData("text/plain"), 10);
            if (!isNaN(playerId)) assign(id, playerId);
        });
        li.appendChild(slot);

        // Position dropdown
        const sel = el("select", "pos");
        const blank = el("option", null, "Pos"); blank.value = "";
        sel.appendChild(blank);
        for (const pos of POSITIONS) {
            const o = el("option", null, pos); o.value = pos;
            sel.appendChild(o);
        }
        sel.value = state.positions[id] || "";
        sel.onchange = () => { state.positions[id] = sel.value; renderPreview(); };
        li.appendChild(sel);

        // Two-way DH toggle (high school): this fielder is also the DH.
        if (!isSpecial) {
            const dh = el("button", "dhbtn" + (state.twoWayDH[id] ? " on" : ""), "DH");
            dh.type = "button";
            dh.title = "High-school two-way player: fields and is the DH (shows as POS/DH)";
            dh.onclick = () => {
                const wasOn = !!state.twoWayDH[id];
                state.twoWayDH = {}; // only one DH per lineup
                if (!wasOn) state.twoWayDH[id] = true;
                renderAll();
            };
            li.appendChild(dh);
        }

        // Clear button
        const clr = el("button", "clear", "✕");
        clr.title = "Clear spot";
        clr.onclick = () => unassign(id);
        li.appendChild(clr);

        ol.appendChild(li);
    }
}

function renderPool() {
    const pool = $("player-pool");
    pool.innerHTML = "";
    const placed = placedPlayerIds();
    if (state.players.length === 0) {
        pool.appendChild(el("span", "hint", "Add players to the roster first."));
        return;
    }
    for (const p of state.players) {
        const chip = el("div", "chip" + (placed.has(p.id) ? " placed" : ""));
        chip.appendChild(el("span", "cnum", p.number || "—"));
        chip.appendChild(el("span", "cname", p.name));
        if (!placed.has(p.id)) {
            chip.draggable = true;
            chip.addEventListener("dragstart", (e) => {
                e.dataTransfer.setData("text/plain", String(p.id));
                e.dataTransfer.effectAllowed = "move";
            });
        }
        pool.appendChild(chip);
    }
    // Pool is a drop target that removes a player from the lineup.
    pool.addEventListener("dragover", (e) => { e.preventDefault(); pool.classList.add("dragover"); });
    pool.addEventListener("dragleave", () => pool.classList.remove("dragover"));
    pool.addEventListener("drop", (e) => {
        e.preventDefault();
        pool.classList.remove("dragover");
        const playerId = parseInt(e.dataTransfer.getData("text/plain"), 10);
        for (const k of Object.keys(state.assignments)) {
            if (state.assignments[k] == playerId) delete state.assignments[k];
        }
        renderAll();
    });
}

function renderPreview() {
    const box = $("card-preview");
    box.innerHTML = "";

    const header = el("div", "cp-header");
    const logo = el("div", "cp-logo");
    if (state.team && state.team.logo_path) {
        logo.style.backgroundImage = `url(${state.team.logo_path})`;
    } else {
        logo.textContent = (state.team && state.team.name ? state.team.name[0] : "?").toUpperCase();
    }
    header.appendChild(logo);
    const hinfo = el("div", "grow");
    hinfo.appendChild(el("div", "cp-team", state.team ? state.team.name : ""));
    if (state.meta.opponent) hinfo.appendChild(el("div", "cp-vs", "vs " + state.meta.opponent));
    const metaBits = [];
    if (state.meta.game_date) metaBits.push(state.meta.game_date);
    if (state.meta.home_away) metaBits.push(state.meta.home_away.toUpperCase());
    if (state.meta.location) metaBits.push(state.meta.location);
    if (metaBits.length) hinfo.appendChild(el("div", "cp-meta", metaBits.join("  •  ")));
    header.appendChild(hinfo);
    box.appendChild(header);

    const table = el("table", "cp-table");
    const thead = el("tr");
    ["#", "No", "Player", "Pos"].forEach((h) => thead.appendChild(el("th", null, h)));
    table.appendChild(thead);
    for (const id of slotIds()) {
        const tr = el("tr");
        tr.appendChild(el("td", "o", id));
        const pid = state.assignments[id];
        const p = pid != null ? state.players.find((pl) => pl.id == pid) : null;
        tr.appendChild(el("td", "n", p ? (p.number || "") : ""));
        tr.appendChild(el("td", "name", p ? p.name : ""));
        let posLabel = state.positions[id] || "";
        if (state.twoWayDH[id]) posLabel = posLabel ? posLabel + "/DH" : "DH";
        tr.appendChild(el("td", "pos", posLabel));
        table.appendChild(tr);
    }
    box.appendChild(table);

    // Substitutes: every roster player not in the batting order.
    const placed = placedPlayerIds();
    const subs = state.players.filter((p) => !placed.has(p.id));
    const subWrap = el("div", "cp-subs");
    subWrap.appendChild(el("div", "cp-subs-h", "SUBSTITUTES"));
    const subGrid = el("div", "cp-subs-grid");
    if (subs.length === 0) {
        subGrid.appendChild(el("span", "cp-sub-none", "None"));
    } else {
        for (const p of subs) {
            const s = el("span", "cp-sub");
            s.appendChild(el("span", "cp-sub-n", p.number || "—"));
            s.appendChild(el("span", null, p.name));
            subGrid.appendChild(s);
        }
    }
    subWrap.appendChild(subGrid);
    box.appendChild(subWrap);

    const foot = el("div", "cp-foot");
    if (state.team && state.team.head_coach) foot.appendChild(el("div", null, "Head Coach: " + state.team.head_coach));
    if (state.team && state.team.assistant_coaches) foot.appendChild(el("div", null, "Assistants: " + state.team.assistant_coaches));
    box.appendChild(foot);
}

// ---------- Player CRUD ----------
function fillPosOptions(sel, includeBlank) {
    sel.innerHTML = "";
    if (includeBlank) { const b = el("option", null, "—"); b.value = ""; sel.appendChild(b); }
    for (const pos of POSITIONS) { const o = el("option", null, pos); o.value = pos; sel.appendChild(o); }
}

let editingPlayerId = null;

function wireRoster() {
    fillPosOptions($("player-pos"), true);
    $("player-form").onsubmit = async (e) => {
        e.preventDefault();
        const payload = {
            number: $("player-number").value.trim(),
            name: $("player-name").value.trim(),
            default_position: $("player-pos").value,
        };
        if (!payload.name) return;
        try {
            if (editingPlayerId) {
                await api("PUT", `/players/${editingPlayerId}`, payload);
                editingPlayerId = null;
                $("player-form").querySelector("button").textContent = "Add";
            } else {
                await api("POST", `/teams/${state.teamId}/players`, payload);
            }
            state.players = await api("GET", `/teams/${state.teamId}/players`);
            $("player-number").value = ""; $("player-name").value = ""; $("player-pos").value = "";
            renderRoster();
            renderAll();
        } catch (err) { setStatus(err.message, true); }
    };
}

function editPlayer(p) {
    editingPlayerId = p.id;
    $("player-number").value = p.number || "";
    $("player-name").value = p.name;
    $("player-pos").value = p.default_position || "";
    $("player-form").querySelector("button").textContent = "Save";
    $("player-name").focus();
}

async function deletePlayer(p) {
    if (!confirm(`Delete ${p.name}?`)) return;
    try {
        await api("DELETE", `/players/${p.id}`);
        state.players = state.players.filter((x) => x.id !== p.id);
        for (const k of Object.keys(state.assignments)) {
            if (state.assignments[k] == p.id) delete state.assignments[k];
        }
        renderRoster();
        renderAll();
    } catch (err) { setStatus(err.message, true); }
}

// ---------- Team actions ----------
function wireTeam() {
    $("team-select").onchange = (e) => selectTeam(e.target.value);
    $("new-team-btn").onclick = async () => {
        const name = prompt("New team name:", "New Team");
        if (!name) return;
        const t = await api("POST", "/teams", { name });
        state.teams.push(t);
        const o = el("option", null, t.name); o.value = t.id;
        $("team-select").appendChild(o);
        $("team-select").value = t.id;
        await selectTeam(t.id);
    };
    $("save-team-btn").onclick = async () => {
        try {
            const payload = {
                name: $("team-name").value.trim(),
                head_coach: $("team-head").value.trim(),
                assistant_coaches: $("team-assts").value.trim(),
            };
            const updated = await api("PUT", `/teams/${state.teamId}`, payload);
            Object.assign(state.team, updated);
            const idx = state.teams.findIndex((t) => t.id === updated.id);
            if (idx >= 0) state.teams[idx] = updated;
            [...$("team-select").options].forEach((o) => { if (o.value == updated.id) o.textContent = updated.name; });
            // Logo upload if a file is chosen.
            const file = $("logo-file").files[0];
            if (file) {
                const fd = new FormData();
                fd.append("logo", file);
                const withLogo = await api("POST", `/teams/${state.teamId}/logo`, fd, true);
                Object.assign(state.team, withLogo);
                $("logo-file").value = "";
            }
            fillTeamFields();
            renderPreview();
            setStatus("Team saved.");
        } catch (err) { setStatus(err.message, true); }
    };
    $("delete-team-btn").onclick = async () => {
        if (state.teams.length <= 1) { setStatus("You need at least one team.", true); return; }
        if (!confirm(`Delete team "${state.team.name}" and all its players and lineups?`)) return;
        await api("DELETE", `/teams/${state.teamId}`);
        state.teams = state.teams.filter((t) => t.id !== state.teamId);
        $("team-select").innerHTML = "";
        for (const t of state.teams) { const o = el("option", null, t.name); o.value = t.id; $("team-select").appendChild(o); }
        await selectTeam(state.teams[0].id);
    };
}

// ---------- Meta wiring ----------
function wireMeta() {
    const bind = (id, key, isCheck) => {
        $(id).addEventListener(isCheck ? "change" : "input", () => {
            state.meta[key] = isCheck ? $(id).checked : $(id).value;
            if (isCheck) renderAll(); else renderPreview();
        });
    };
    bind("meta-opponent", "opponent");
    bind("meta-date", "game_date");
    bind("meta-location", "location");
    bind("meta-homeaway", "home_away");
    bind("meta-name", "name");
    bind("meta-dh", "use_dh", true);
    bind("meta-eh", "use_eh", true);
}

// ---------- Lineup actions ----------
function wireLineup() {
    $("lineup-select").onchange = (e) => {
        const id = e.target.value;
        if (!id) newLineup(), renderAll();
        else loadLineup(id);
    };
    $("save-lineup-btn").onclick = async () => {
        try {
            const payload = lineupPayload();
            let detail;
            if (state.lineupId) {
                detail = await api("PUT", `/lineups/${state.lineupId}`, payload);
            } else {
                detail = await api("POST", `/teams/${state.teamId}/lineups`, payload);
            }
            state.lineupId = detail.id;
            state.lineups = await api("GET", `/teams/${state.teamId}/lineups`);
            fillLineupSelect();
            setStatus("Lineup saved.");
        } catch (err) { setStatus(err.message, true); }
    };
    $("delete-lineup-btn").onclick = async () => {
        if (!state.lineupId) { newLineup(); renderAll(); return; }
        if (!confirm("Delete this saved lineup?")) return;
        await api("DELETE", `/lineups/${state.lineupId}`);
        state.lineups = state.lineups.filter((l) => l.id !== state.lineupId);
        newLineup();
        fillLineupSelect();
        renderAll();
        setStatus("Lineup deleted.");
    };
    $("pdf-btn").onclick = () => $("pdf-dialog").showModal();
    $("pdf-dialog").addEventListener("close", async () => {
        if ($("pdf-dialog").returnValue !== "ok") return;
        // Ensure the lineup is saved before generating the PDF.
        try {
            // Always persist the current state so the PDF matches what's on screen.
            const payload = lineupPayload();
            const detail = state.lineupId
                ? await api("PUT", `/lineups/${state.lineupId}`, payload)
                : await api("POST", `/teams/${state.teamId}/lineups`, payload);
            state.lineupId = detail.id;
            state.lineups = await api("GET", `/teams/${state.teamId}/lineups`);
            fillLineupSelect();
            const q = new URLSearchParams({
                coach: $("copies-coach").value || "0",
                scorekeeper: $("copies-score").value || "0",
                self: $("copies-self").value || "0",
                umpire: $("copies-ump").value || "0",
            });
            window.open(`/api/lineups/${state.lineupId}/pdf?${q.toString()}`, "_blank");
        } catch (err) { setStatus(err.message, true); }
    });
}

// ---------- Top-level wiring ----------
function wireApp() {
    $("logout-btn").onclick = async () => {
        await api("POST", "/logout");
        state.user = null;
        showAuth();
    };
    wireTeam();
    wireRoster();
    wireMeta();
    wireLineup();
}

async function init() {
    wireAuth();
    wireApp();
    try {
        state.user = await api("GET", "/me");
        await bootApp();
    } catch (_) {
        showAuth();
    }
}

document.addEventListener("DOMContentLoaded", init);

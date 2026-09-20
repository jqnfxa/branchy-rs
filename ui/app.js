// The interface.
//
// Draws a snapshot and sends command lines back. It computes nothing about the
// graph: status, tier, the queue and any cycles all arrive already worked out.
// The one thing worked out here is where things sit on screen, which is a
// rendering concern and lives in layout.js.

(function (global) {
  "use strict";

  var B = global.Branchy;
  var I18N = B.I18N;
  var LANGS = B.LANGS;
  var langById = {};
  LANGS.forEach(function (l) {
    langById[l.id] = l;
  });

  var LAYOUTS = ["radial", "layered", "web"];
  var DEFAULTS = {
    lang: "en",
    layout: "radial",
    skin: "dark",
    dockSide: "left",
    motion: "full",
  };

  var backend = null;
  var snap = { areas: [], nodes: [], queue: [], cycles: [], tally: {} };
  var byId = {};
  var areaById = {};
  var LAY = null;

  var state = {
    lang: DEFAULTS.lang,
    layout: DEFAULTS.layout,
    skin: DEFAULTS.skin,
    dockSide: DEFAULTS.dockSide,
    motion: DEFAULTS.motion,
    visible: {},
    focus: null,
    selected: null,
    trace: true,
    query: "",
    regex: false,
    view: { x: 0, y: 0, k: 1 },
  };

  /* ---------- preferences: per device, never part of the graph ---------- */

  function loadPrefs() {
    var stored = {};
    try {
      var raw = localStorage.getItem("branchy.prefs");
      if (raw) stored = JSON.parse(raw) || {};
    } catch (e) {
      stored = {};
    }
    Object.keys(DEFAULTS).forEach(function (key) {
      if (stored[key] !== undefined) state[key] = stored[key];
    });
    if (!langById[state.lang]) state.lang = "en";
  }

  function savePrefs() {
    try {
      var out = {};
      Object.keys(DEFAULTS).forEach(function (key) {
        out[key] = state[key];
      });
      localStorage.setItem("branchy.prefs", JSON.stringify(out));
    } catch (e) {
      /* private window or blocked storage: the app still works */
    }
  }

  /* ---------- translation ---------- */

  function plural(locale, n, forms) {
    try {
      var category = new Intl.PluralRules(locale).select(n);
      if (forms[category] !== undefined) return forms[category];
    } catch (e) {
      /* fall through */
    }
    return forms.other !== undefined ? forms.other : forms.one;
  }

  function num(n) {
    try {
      return new Intl.NumberFormat(langById[state.lang].locale).format(n);
    } catch (e) {
      return String(n);
    }
  }

  function t(key, vars) {
    var lang = langById[state.lang] ? state.lang : "en";
    var value = I18N[lang][key];
    if (value === undefined) value = I18N.en[key];
    if (value === undefined) return key;
    if (typeof value === "object") {
      value = plural(langById[lang].locale, (vars && vars.n) || 0, value);
    }
    return String(value).replace(/\{(\w+)\}/g, function (whole, name) {
      if (!vars || vars[name] === undefined) return whole;
      return typeof vars[name] === "number" ? num(vars[name]) : vars[name];
    });
  }

  var loadedFonts = {};
  function ensureFont(lang) {
    var family = langById[lang] && langById[lang].font;
    if (!family || loadedFonts[family]) return;
    loadedFonts[family] = true;
    var link = document.createElement("link");
    link.rel = "stylesheet";
    link.href =
      "https://fonts.googleapis.com/css2?family=" + family + "&display=swap";
    document.head.appendChild(link);
  }

  /* ---------- svg plumbing ---------- */

  var NS = "http://www.w3.org/2000/svg";
  var stage, canvas, world, gDecor, gLinks, gFx, gNodes, inspector, dlg;
  var palette, palIn, palOut, palSugg, queueEl, queueBtn, qInput, searchWrap;
  var dockEl, rowEl, toastEl, bannerEl;

  function el(tag, attrs) {
    var node = document.createElementNS(NS, tag);
    if (attrs) {
      for (var key in attrs) node.setAttribute(key, attrs[key]);
    }
    return node;
  }

  function colorOf(areaId) {
    var area = areaById[areaId];
    return area ? area.color : "var(--text-dim)";
  }

  function shownNodes() {
    return snap.nodes.filter(function (node) {
      return state.focus ? node.area === state.focus : state.visible[node.area];
    });
  }

  function isShown(id) {
    var node = byId[id];
    if (!node) return false;
    return state.focus ? node.area === state.focus : state.visible[node.area];
  }

  function motionOn() {
    if (state.motion !== "full") return false;
    try {
      return !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    } catch (e) {
      return true;
    }
  }

  /* ---------- talking to the backend ---------- */

  function refresh() {
    return backend.snapshot().then(function (next) {
      adopt(next);
      render();
      paintChrome();
    });
  }

  function adopt(next) {
    snap = next;
    byId = {};
    areaById = {};
    snap.areas.forEach(function (area) {
      areaById[area.id] = area;
      if (state.visible[area.id] === undefined) state.visible[area.id] = true;
    });
    snap.nodes.forEach(function (node) {
      byId[node.id] = node;
    });
    if (state.selected && !byId[state.selected]) state.selected = null;
    if (state.focus && !areaById[state.focus]) state.focus = null;
  }

  // Runs a command line, then redraws from whatever the backend now holds.
  // Nothing is assumed about the outcome: the graph is the authority, so the
  // interface asks it again rather than guessing.
  function execute(line) {
    var before = {};
    snap.nodes.forEach(function (node) {
      before[node.id] = node.status;
    });

    return backend
      .execute(line)
      .then(function () {
        return backend.snapshot();
      })
      .then(function (next) {
        adopt(next);
        render();
        paintChrome();
        celebrate(before);
        return true;
      })
      .catch(function (error) {
        toast(message(error), true);
        return false;
      });
  }

  function message(error) {
    var text = error && error.message ? error.message : String(error);
    if (text.indexOf("readonly:") === 0) return text.slice(9);
    return text;
  }

  // The one celebratory moment: anything that just became available flashes.
  function celebrate(before) {
    if (!motionOn()) return;
    snap.nodes.forEach(function (node) {
      if (before[node.id] === "locked" && node.status === "available") {
        var group = nodeEl(node.id);
        if (group) group.classList.add("just-freed");
      }
      if (before[node.id] === "available" && node.status === "done") {
        burstAt(node.id, colorOf(node.area));
      }
    });
  }

  /* ---------- rendering ---------- */

  function linkPath(a, b, cross) {
    if (LAY.layered) {
      var dx = Math.max(38, Math.abs(b.x - a.x) * 0.45);
      return (
        "M" + a.x + " " + a.y + " C" + (a.x + dx) + " " + a.y + "," +
        (b.x - dx) + " " + b.y + "," + b.x + " " + b.y
      );
    }
    var mx = (a.x + b.x) / 2;
    var my = (a.y + b.y) / 2;
    var pull = cross ? 0.55 : 0.14;
    return (
      "M" + a.x + " " + a.y + " Q" + mx * (1 - pull) + " " + my * (1 - pull) +
      " " + b.x + " " + b.y
    );
  }

  function glyphFor(status) {
    var glyph;
    if (status === "done") {
      glyph = el("path", { d: "M-5 0.2 L-1.6 3.6 L5.4 -3.8", fill: "none", "stroke-width": "2" });
    } else if (status === "available") {
      glyph = el("path", { d: "M0 -4.8 L4.8 0 L0 4.8 L-4.8 0 Z" });
    } else if (status === "cyclic") {
      glyph = el("path", {
        d: "M-4 -4 L4 4 M4 -4 L-4 4",
        fill: "none",
        "stroke-width": "1.8",
      });
    } else {
      glyph = el("g");
      glyph.appendChild(el("rect", { x: -3.6, y: -0.4, width: 7.2, height: 5.4, rx: 1, fill: "none", "stroke-width": "1.5" }));
      glyph.appendChild(el("path", { d: "M-2 -0.4 V-2.2 a2 2 0 0 1 4 0 V-0.4", fill: "none", "stroke-width": "1.5" }));
    }
    glyph.setAttribute("class", "glyph");
    return glyph;
  }

  function drawHub() {
    var total = snap.tally.total || 0;
    var done = snap.tally.done || 0;
    var fraction = total ? done / total : 0;
    var radius = 44;
    var circumference = 2 * Math.PI * radius;

    var group = el("g", { class: "you" });
    group.appendChild(el("circle", { class: "plate", cx: 0, cy: 0, r: 34 }));
    group.appendChild(el("circle", { class: "track", cx: 0, cy: 0, r: radius }));
    group.appendChild(
      el("circle", {
        class: "prog",
        cx: 0,
        cy: 0,
        r: radius,
        "stroke-dasharray": circumference,
        "stroke-dashoffset": circumference * (1 - fraction),
        transform: "rotate(-90)",
      })
    );
    var who = el("text", { class: "who", x: 0, y: -4 });
    who.textContent = "YOU";
    var pct = el("text", { class: "pct", x: 0, y: 11 });
    pct.textContent = Math.round(fraction * 100) + "%";
    group.appendChild(who);
    group.appendChild(pct);
    return group;
  }

  function render() {
    var areas = snap.areas.filter(function (area) {
      return state.focus ? area.id === state.focus : state.visible[area.id];
    });
    LAY = B.layout(state.layout, areas, shownNodes());

    gDecor.textContent = "";
    gLinks.textContent = "";
    gNodes.textContent = "";
    gFx.textContent = "";

    var positions = LAY.positions;
    var K = B.layoutConstants;

    if (!LAY.layered) {
      for (var ring = 0; ring <= (LAY.rings || 0); ring++) {
        gDecor.appendChild(el("circle", { class: "ring", cx: 0, cy: 0, r: K.R0 + ring * K.RING }));
      }
      gDecor.appendChild(drawHub());
      LAY.sectors.forEach(function (sector) {
        var label = el("text", {
          x: Math.cos(sector.mid) * sector.r,
          y: Math.sin(sector.mid) * sector.r,
          class: "sector-label",
          fill: sector.area.color,
          "text-anchor": "middle",
        });
        label.textContent = sector.area.name;
        gDecor.appendChild(label);
      });
    } else {
      LAY.sectors.forEach(function (sector) {
        var label = el("text", {
          x: -58,
          y: (sector.band.y0 + sector.band.y1) / 2,
          class: "sector-label",
          fill: sector.area.color,
          "text-anchor": "end",
        });
        label.textContent = sector.area.name;
        gDecor.appendChild(label);
      });
    }

    snap.nodes.forEach(function (node) {
      if (!isShown(node.id)) return;
      node.prereqs.forEach(function (prereqId) {
        var prereq = byId[prereqId];
        if (!prereq || !isShown(prereqId)) return;
        var from = positions[prereqId];
        var to = positions[node.id];
        if (!from || !to) return;
        var cross = prereq.area !== node.area;
        var kind = prereq.done ? (node.done ? "done" : "open") : "locked";
        var path = el("path", {
          class: "link state-" + kind + (cross ? " cross" : ""),
          d: linkPath(from, to, cross),
        });
        if (kind !== "locked") path.setAttribute("stroke", colorOf(prereq.area));
        path.dataset.from = prereqId;
        path.dataset.to = node.id;
        gLinks.appendChild(path);
      });
    });

    shownNodes().forEach(function (node) {
      var at = positions[node.id];
      if (!at) return;
      var group = el("g", {
        class: "node " + node.status + (state.selected === node.id ? " sel" : ""),
        transform: "translate(" + at.x + "," + at.y + ")",
        tabindex: "0",
        role: "button",
      });
      group.style.setProperty("--c", colorOf(node.area));
      group.dataset.id = node.id;
      group.appendChild(el("circle", { class: "halo", cx: 0, cy: 0, r: 24 }));
      group.appendChild(el("circle", { class: "disc", cx: 0, cy: 0, r: 18 }));
      group.appendChild(glyphFor(node.status));

      var label = el("text", { class: "label", x: 0, y: 34 });
      label.textContent = node.name;
      group.appendChild(label);
      group.appendChild(el("circle", { class: "hit", cx: 0, cy: 0, r: 26 }));

      var title = el("title");
      title.textContent = node.name + " — " + t("st." + node.status);
      group.appendChild(title);

      group.addEventListener("click", function (ev) {
        ev.stopPropagation();
        select(node.id);
      });
      group.addEventListener("keydown", function (ev) {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          select(node.id);
        }
      });
      gNodes.appendChild(group);
    });

    updateEmphasis();
    applyTransform();
  }

  function nodeEl(id) {
    return Array.prototype.filter.call(gNodes.children, function (group) {
      return group.dataset.id === id;
    })[0];
  }

  function burstAt(id, color) {
    if (!LAY || !LAY.positions[id]) return;
    var at = LAY.positions[id];
    var ring = el("circle", { class: "burst", cx: at.x, cy: at.y, r: 18, stroke: color });
    gFx.appendChild(ring);
    var started = performance.now();
    (function step(now) {
      var k = Math.min(1, (now - started) / 620);
      ring.setAttribute("r", 18 + k * 52);
      ring.setAttribute("stroke-opacity", String(1 - k));
      ring.setAttribute("stroke-width", String(3 * (1 - k) + 0.6));
      if (k < 1) requestAnimationFrame(step);
      else if (ring.parentNode) ring.parentNode.removeChild(ring);
    })(started);
  }

  /* ---------- camera ---------- */

  var animation = null;

  function applyTransform() {
    var view = state.view;
    world.setAttribute(
      "transform",
      "translate(" + view.x + "," + view.y + ") scale(" + view.k + ")"
    );
    document.getElementById("zLvl").textContent = Math.round(view.k * 100) + "%";
    gNodes.classList.toggle("hide-labels", view.k < 0.52);
  }

  function tweenTo(target, ms) {
    if (animation) cancelAnimationFrame(animation);
    if (!motionOn() || !ms) {
      state.view.x = target.x;
      state.view.y = target.y;
      state.view.k = target.k;
      applyTransform();
      return;
    }
    var from = { x: state.view.x, y: state.view.y, k: state.view.k };
    var started = performance.now();
    (function step(now) {
      var p = Math.min(1, (now - started) / ms);
      var e = p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2;
      // scale interpolates geometrically, which is what reads as smooth
      state.view.k = from.k * Math.pow(target.k / from.k, e);
      state.view.x = from.x + (target.x - from.x) * e;
      state.view.y = from.y + (target.y - from.y) * e;
      applyTransform();
      if (p < 1) animation = requestAnimationFrame(step);
    })(performance.now());
  }

  function fitTarget() {
    var ids = Object.keys(LAY.positions);
    if (!ids.length) return null;
    var pad = 96;
    var x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    ids.forEach(function (id) {
      var at = LAY.positions[id];
      x0 = Math.min(x0, at.x); x1 = Math.max(x1, at.x);
      y0 = Math.min(y0, at.y); y1 = Math.max(y1, at.y);
    });
    if (!LAY.layered) {
      x0 = Math.min(x0, -60); x1 = Math.max(x1, 60);
      y0 = Math.min(y0, -60); y1 = Math.max(y1, 60);
    }
    x0 -= pad; y0 -= pad; x1 += pad; y1 += pad;
    var w = stage.clientWidth, h = stage.clientHeight;
    var k = Math.max(0.28, Math.min(1.6, Math.min(w / (x1 - x0), h / (y1 - y0))));
    return { k: k, x: w / 2 - ((x0 + x1) / 2) * k, y: h / 2 - ((y0 + y1) / 2) * k };
  }

  function fit(ms) {
    var target = fitTarget();
    if (target) tweenTo(target, ms === undefined ? 420 : ms);
  }

  function flyTo(id) {
    if (!LAY || !LAY.positions[id]) return;
    var at = LAY.positions[id];
    var k = Math.min(Math.max(state.view.k, 1.15), 1.6);
    tweenTo(
      { k: k, x: stage.clientWidth / 2 - at.x * k, y: stage.clientHeight / 2 - at.y * k },
      560
    );
  }

  function wirePanZoom() {
    var drag = null;
    stage.addEventListener("pointerdown", function (ev) {
      if (ev.target.closest(".zoom, .queue")) return;
      if (animation) cancelAnimationFrame(animation);
      drag = { x: ev.clientX, y: ev.clientY, vx: state.view.x, vy: state.view.y, moved: false };
      stage.classList.add("grabbing");
      stage.setPointerCapture(ev.pointerId);
    });
    stage.addEventListener("pointermove", function (ev) {
      if (!drag) return;
      var dx = ev.clientX - drag.x;
      var dy = ev.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      state.view.x = drag.vx + dx;
      state.view.y = drag.vy + dy;
      applyTransform();
    });
    stage.addEventListener("pointerup", function (ev) {
      if (drag && !drag.moved && !ev.target.closest(".node")) select(null);
      drag = null;
      stage.classList.remove("grabbing");
    });
    stage.addEventListener("pointercancel", function () {
      drag = null;
      stage.classList.remove("grabbing");
    });
    stage.addEventListener(
      "wheel",
      function (ev) {
        ev.preventDefault();
        var box = stage.getBoundingClientRect();
        zoomAt(ev.clientX - box.left, ev.clientY - box.top, Math.exp(-ev.deltaY * 0.0016));
      },
      { passive: false }
    );

    // two fingers on a touchscreen: Android is a target, so this is not optional
    var pinch = null;
    stage.addEventListener("touchstart", function (ev) {
      if (ev.touches.length === 2) {
        pinch = spread(ev.touches);
        if (animation) cancelAnimationFrame(animation);
      }
    }, { passive: true });
    stage.addEventListener("touchmove", function (ev) {
      if (ev.touches.length !== 2 || !pinch) return;
      ev.preventDefault();
      var now = spread(ev.touches);
      var box = stage.getBoundingClientRect();
      zoomAt(now.x - box.left, now.y - box.top, now.d / pinch.d);
      pinch = now;
    }, { passive: false });
    stage.addEventListener("touchend", function () { pinch = null; });

    function spread(touches) {
      var dx = touches[0].clientX - touches[1].clientX;
      var dy = touches[0].clientY - touches[1].clientY;
      return {
        d: Math.max(1, Math.hypot(dx, dy)),
        x: (touches[0].clientX + touches[1].clientX) / 2,
        y: (touches[0].clientY + touches[1].clientY) / 2,
      };
    }

    function zoomAt(mx, my, factor) {
      if (animation) cancelAnimationFrame(animation);
      var view = state.view;
      var k = Math.max(0.22, Math.min(2.6, view.k * factor));
      view.x = mx - (mx - view.x) * (k / view.k);
      view.y = my - (my - view.y) * (k / view.k);
      view.k = k;
      applyTransform();
    }

    document.getElementById("zIn").onclick = function () {
      zoomAt(stage.clientWidth / 2, stage.clientHeight / 2, 1.3);
    };
    document.getElementById("zOut").onclick = function () {
      zoomAt(stage.clientWidth / 2, stage.clientHeight / 2, 0.77);
    };
    document.getElementById("zFit").onclick = function () { fit(); };
  }

  /* ---------- emphasis: search wins, otherwise trace the selection ---------- */

  function queryHits() {
    var query = state.query.trim();
    searchWrap.classList.remove("invalid");
    if (!query) return null;
    var test;
    if (state.regex) {
      try {
        var re = new RegExp(query, "i");
        test = function (text) { return re.test(text); };
      } catch (e) {
        searchWrap.classList.add("invalid");
        return null;
      }
    } else {
      var lower = query.toLowerCase();
      test = function (text) { return text.toLowerCase().indexOf(lower) !== -1; };
    }
    var hits = {};
    snap.nodes.forEach(function (node) {
      var area = areaById[node.area];
      if (test(node.name) || test(node.note) || (area && test(area.name))) {
        hits[node.id] = true;
      }
    });
    return hits;
  }

  function closureOf(id) {
    var set = {};
    set[id] = true;
    var stack = [id];
    while (stack.length) {
      var current = stack.pop();
      (byId[current] ? byId[current].prereqs : []).forEach(function (p) {
        if (byId[p] && !set[p]) { set[p] = true; stack.push(p); }
      });
    }
    stack = [id];
    var seen = {};
    seen[id] = true;
    while (stack.length) {
      var at = stack.pop();
      (byId[at] ? byId[at].dependents : []).forEach(function (d) {
        if (!seen[d]) { seen[d] = true; set[d] = true; stack.push(d); }
      });
    }
    return set;
  }

  function updateEmphasis() {
    var hits = queryHits();
    var path = null;
    if (!hits && state.selected && state.trace) path = closureOf(state.selected);
    var set = hits || path;

    Array.prototype.forEach.call(gNodes.children, function (group) {
      group.classList.toggle("faded", !!set && !set[group.dataset.id]);
    });
    Array.prototype.forEach.call(gLinks.children, function (link) {
      var on = !set || (set[link.dataset.from] && set[link.dataset.to]);
      link.classList.toggle("faded", !on);
      link.classList.toggle("on-path", !!path && on);
    });
  }

  /* ---------- selection and inspector ---------- */

  function select(id, fly, trace) {
    state.selected = id;
    state.trace = trace !== false;
    Array.prototype.forEach.call(gNodes.children, function (group) {
      group.classList.toggle("sel", group.dataset.id === id);
    });
    updateEmphasis();
    if (!id) {
      inspector.hidden = true;
      return;
    }
    drawInspector(byId[id]);
    inspector.hidden = false;
    if (fly !== false) flyTo(id);
  }

  function drawInspector(node) {
    if (!node) return;
    var area = areaById[node.area] || { name: "?", color: "var(--text-dim)" };
    inspector.innerHTML = "";
    inspector.style.setProperty("--c", area.color);

    var head = document.createElement("div");
    head.className = "insp-head";
    var chip = document.createElement("span");
    chip.className = "chip";
    chip.style.setProperty("--c", area.color);
    chip.textContent = area.name;
    var close = document.createElement("button");
    close.className = "xbtn";
    close.type = "button";
    close.textContent = "×";
    close.setAttribute("aria-label", "Close");
    close.onclick = function () { select(null); };
    head.appendChild(chip);
    head.appendChild(close);
    inspector.appendChild(head);

    var body = document.createElement("div");
    body.className = "insp";

    var title = document.createElement("h3");
    title.textContent = node.name;
    body.appendChild(title);

    var meta = document.createElement("div");
    meta.className = "meta";
    var tierText = node.tier === null || node.tier === undefined
      ? t("st.cyclic")
      : t("tier", { t: node.tier });
    [tierText, t("priority", { p: node.priority }), t("st." + node.status)].forEach(function (text) {
      var span = document.createElement("span");
      span.textContent = text;
      meta.appendChild(span);
    });
    body.appendChild(meta);

    if (node.note) {
      var note = document.createElement("p");
      note.className = "note";
      note.textContent = node.note;
      body.appendChild(note);
    }

    body.appendChild(depList(t("needs"), node.prereqs, true, node));
    body.appendChild(depList(t("unlocksLbl"), node.dependents, false, node));

    var action = document.createElement("button");
    action.className = "act" + (node.status === "available" ? " primary" : "");
    action.type = "button";
    action.style.setProperty("--c", area.color);
    if (node.status === "done") {
      action.textContent = t("act.undo");
      action.onclick = function () { execute("undone " + node.id); };
    } else if (node.status === "available") {
      action.textContent = t("act.unlock");
      action.onclick = function () { execute("done " + node.id); };
    } else if (node.status === "cyclic") {
      action.textContent = t("st.cyclic");
      action.disabled = true;
    } else {
      var missing = node.prereqs.filter(function (p) {
        return byId[p] && !byId[p].done;
      }).length;
      action.textContent = t("act.locked", { n: missing });
      action.disabled = true;
    }
    body.appendChild(action);
    inspector.appendChild(body);
  }

  function depList(title, ids, showMet, owner) {
    var wrap = document.createElement("div");
    var heading = document.createElement("h4");
    heading.textContent = title;
    wrap.appendChild(heading);

    var list = document.createElement("ul");
    list.className = "deps";
    if (!ids.length) {
      var empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = showMet ? t("noNeeds") : t("noUnlocks");
      list.appendChild(empty);
    }
    ids.forEach(function (id) {
      var other = byId[id];
      if (!other) return;
      var row = document.createElement("li");
      row.className = showMet ? (other.done ? "met" : "unmet") : "";
      var mark = document.createElement("span");
      mark.className = "mark";
      mark.textContent = showMet ? (other.done ? "✓" : "✕") : "→";
      var name = document.createElement("span");
      name.className = "nm";
      name.textContent = other.name;
      row.appendChild(mark);
      row.appendChild(name);
      if (owner && other.area !== owner.area) {
        var from = document.createElement("span");
        from.className = "from";
        from.style.color = colorOf(other.area);
        var label = areaById[other.area];
        from.textContent = label ? label.name.split(" ")[0] : "";
        row.appendChild(from);
      }
      row.onclick = function () {
        if (!isShown(id)) {
          state.focus = null;
          state.visible[other.area] = true;
          paintAreas();
          render();
        }
        select(id);
      };
      list.appendChild(row);
    });
    wrap.appendChild(list);
    return wrap;
  }

  /* ---------- chrome ---------- */

  function paintAreas() {
    var host = document.getElementById("areas");
    host.innerHTML = "";
    snap.areas.forEach(function (area) {
      var button = document.createElement("button");
      button.className = "area";
      button.type = "button";
      button.style.setProperty("--c", area.color);
      button.dataset.on = state.focus
        ? String(area.id === state.focus)
        : String(!!state.visible[area.id]);
      button.dataset.focus = String(state.focus === area.id);

      var swatch = document.createElement("span");
      swatch.className = "swatch";
      var name = document.createElement("span");
      name.className = "nm";
      name.textContent = area.name;
      var count = document.createElement("span");
      count.className = "ct";
      count.textContent = num(area.done) + "/" + num(area.total);
      var bar = document.createElement("span");
      bar.className = "bar";
      var fill = document.createElement("i");
      fill.style.width = (area.total ? (area.done / area.total) * 100 : 0) + "%";
      bar.appendChild(fill);

      button.appendChild(swatch);
      button.appendChild(name);
      button.appendChild(count);
      button.appendChild(bar);

      swatch.addEventListener("click", function (ev) {
        ev.stopPropagation();
        state.focus = null;
        state.visible[area.id] = !state.visible[area.id];
        paintAreas();
        render();
        fit();
      });
      button.addEventListener("click", function () {
        if (state.focus === area.id) {
          state.focus = null;
          setLayout(state.layout, false);
        } else {
          state.focus = area.id;
          setLayout("layered", false);
        }
        paintAreas();
        render();
        fit();
      });
      host.appendChild(button);
    });
  }

  function paintTally() {
    document.getElementById("tally").innerHTML = t("tally", {
      done: snap.tally.done || 0,
      total: snap.tally.total || 0,
      avail: snap.tally.available || 0,
    });
  }

  function paintLayoutSeg() {
    var seg = document.getElementById("layoutSeg");
    seg.innerHTML = "";
    LAYOUTS.forEach(function (id, index) {
      var button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-pressed", String(state.layout === id));
      button.appendChild(document.createTextNode(t("layout." + id)));
      var hint = document.createElement("span");
      hint.className = "kb";
      hint.textContent = String(index + 1);
      button.appendChild(hint);
      button.onclick = function () { setLayout(id, true); };
      seg.appendChild(button);
    });
  }

  function setLayout(id, persist) {
    state.layout = id;
    if (persist) savePrefs();
    paintLayoutSeg();
    render();
    fit();
    if (dlg.open) paintSettings();
  }

  function paintQueue() {
    var list = document.getElementById("qlist");
    list.innerHTML = "";
    snap.queue.forEach(function (id) {
      var node = byId[id];
      if (!node) return;
      var row = document.createElement("li");
      row.style.setProperty("--c", colorOf(node.area));
      var priority = document.createElement("span");
      priority.className = "pri";
      priority.textContent = num(node.priority);
      var middle = document.createElement("span");
      var name = document.createElement("div");
      name.className = "nm";
      name.textContent = node.name;
      var sub = document.createElement("div");
      sub.className = "sub";
      sub.textContent = t("q.meta", {
        t: node.tier === null || node.tier === undefined ? 0 : node.tier,
        n: node.dependents.length,
      });
      middle.appendChild(name);
      middle.appendChild(sub);
      var tag = document.createElement("span");
      tag.className = "tag";
      var area = areaById[node.area];
      tag.textContent = area ? area.name.split(" ")[0] : "";
      row.appendChild(priority);
      row.appendChild(middle);
      row.appendChild(tag);
      row.onclick = function () {
        if (!isShown(id)) {
          state.focus = null;
          state.visible[node.area] = true;
          paintAreas();
          render();
        }
        select(id);
      };
      list.appendChild(row);
    });
  }

  function paintChrome() {
    document.documentElement.lang = langById[state.lang].locale;
    document.getElementById("lblDirections").textContent = t("directions");
    document.getElementById("allBtn").textContent = t("all");
    document.getElementById("lblKeys").textContent = t("keys");
    queueBtn.textContent = t("queue");
    document.getElementById("qTitle").textContent = t("q.title");
    document.getElementById("qBlurb").textContent = t("q.blurb");
    document.getElementById("setTitle").textContent = t("set.title");
    qInput.placeholder = state.regex ? t("search.phRx") : t("search.ph");
    palIn.placeholder = t("cmd.ph");
    document.getElementById("palHint").textContent = t("cmd.hint");

    var hints = [
      ["k.command", ":"], ["k.search", "/"], ["k.fit", "F"],
      ["k.queue", "Q"], ["k.layouts", "1 2 3"], ["k.settings", "S"],
      ["k.close", "Esc"],
    ];
    var hintHost = document.getElementById("keyHints");
    hintHost.innerHTML = "";
    hints.forEach(function (pair) {
      var row = document.createElement("div");
      var label = document.createElement("span");
      label.textContent = t(pair[0]);
      var key = document.createElement("kbd");
      key.textContent = pair[1];
      row.appendChild(label);
      row.appendChild(key);
      hintHost.appendChild(row);
    });

    var legend = document.getElementById("legend");
    legend.innerHTML = "";
    [["l-done", "lg.done"], ["l-avail", "lg.available"],
     ["l-locked", "lg.locked"], ["l-cross", "lg.cross"]].forEach(function (pair) {
      var item = document.createElement("span");
      var swatch = document.createElement("i");
      swatch.className = pair[0];
      item.appendChild(swatch);
      item.appendChild(document.createTextNode(t(pair[1])));
      legend.appendChild(item);
    });

    var footer = document.getElementById("footer");
    footer.innerHTML = "";
    ["ft.pan", "ft.zoom", "ft.click", "ft.isolate"].forEach(function (key) {
      var span = document.createElement("span");
      span.textContent = t(key);
      footer.appendChild(span);
    });

    paintLayoutSeg();
    paintTally();
    paintAreas();
    paintQueue();
    if (state.selected) drawInspector(byId[state.selected]);
  }

  /* ---------- settings ---------- */

  function choiceRow(label, help, options, current, pick) {
    var field = document.createElement("div");
    field.className = "field";
    var heading = document.createElement("label");
    heading.textContent = label;
    field.appendChild(heading);
    var box = document.createElement("div");
    box.className = "choices";
    options.forEach(function (option) {
      var button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-pressed", String(option.id === current));
      button.appendChild(document.createTextNode(option.label));
      if (option.kb) {
        var hint = document.createElement("span");
        hint.className = "kb";
        hint.textContent = option.kb;
        button.appendChild(hint);
      }
      button.onclick = function () { pick(option.id); };
      box.appendChild(button);
    });
    field.appendChild(box);
    if (help) {
      var note = document.createElement("p");
      note.className = "help";
      note.textContent = help;
      field.appendChild(note);
    }
    return field;
  }

  function paintSettings() {
    var body = document.getElementById("setBody");
    body.innerHTML = "";
    document.getElementById("setTitle").textContent = t("set.title");

    var field = document.createElement("div");
    field.className = "field";
    var label = document.createElement("label");
    label.setAttribute("for", "langSel");
    label.textContent = t("set.language");
    var select = document.createElement("select");
    select.id = "langSel";
    LANGS.forEach(function (lang) {
      var option = document.createElement("option");
      option.value = lang.id;
      option.textContent = lang.label;
      if (lang.id === state.lang) option.selected = true;
      select.appendChild(option);
    });
    select.onchange = function () { setLang(select.value); };
    var help = document.createElement("p");
    help.className = "help";
    help.textContent = t("set.langNote");
    field.appendChild(label);
    field.appendChild(select);
    field.appendChild(help);
    body.appendChild(field);

    body.appendChild(choiceRow(
      t("set.layout"), t("set.layoutNote"),
      LAYOUTS.map(function (id, i) {
        return { id: id, label: t("layout." + id), kb: String(i + 1) };
      }),
      state.layout,
      function (id) { setLayout(id, true); }
    ));

    body.appendChild(choiceRow(
      t("set.theme"), null,
      [{ id: "dark", label: t("th.dark") }, { id: "light", label: t("th.light") }],
      state.skin,
      function (id) {
        state.skin = id;
        document.body.dataset.skin = id;
        savePrefs();
        paintSettings();
      }
    ));

    body.appendChild(choiceRow(
      t("set.dock"), null,
      [{ id: "left", label: t("dk.left") }, { id: "right", label: t("dk.right") }],
      state.dockSide,
      function (id) {
        state.dockSide = id;
        rowEl.classList.toggle("dock-right", id === "right");
        savePrefs();
        paintSettings();
        requestAnimationFrame(function () { fit(0); });
      }
    ));

    body.appendChild(choiceRow(
      t("set.motion"), t("set.motionNote"),
      [{ id: "full", label: t("mo.full") }, { id: "reduced", label: t("mo.reduced") }],
      state.motion,
      function (id) {
        state.motion = id;
        document.body.dataset.motion = id;
        savePrefs();
        paintSettings();
      }
    ));
  }

  function setLang(id) {
    if (!langById[id]) return;
    state.lang = id;
    ensureFont(id);
    savePrefs();
    paintChrome();
    render();
    if (dlg.open) paintSettings();
  }

  /* ---------- command palette ---------- */

  function openPalette(seed) {
    palette.hidden = false;
    palIn.value = seed || "";
    palIn.focus();
    refreshPalette();
  }

  function closePalette() {
    palette.hidden = true;
    palIn.value = "";
    palIn.blur();
  }

  // No client-side validation of the command: the graph is the authority on
  // what it will accept, so the line goes over and the refusal comes back.
  function refreshPalette() {
    palSugg.innerHTML = "";
    var text = palIn.value.trim();
    palOut.className = "pal-out";
    palOut.textContent = text ? "↵ " + text : "";

    var tail = palIn.value.split(/[\s,]+/).pop().toLowerCase();
    if (tail.length >= 2) {
      snap.nodes
        .filter(function (node) {
          return (
            node.id.indexOf(tail) === 0 ||
            node.name.toLowerCase().indexOf(tail) !== -1
          );
        })
        .slice(0, 8)
        .forEach(function (node) { suggest(node.id, node.name); });
    }
  }

  function suggest(id, name) {
    var item = document.createElement("li");
    item.textContent = id + "  " + name;
    item.onclick = function () {
      var parts = palIn.value.split(/([\s,]+)/);
      parts[parts.length - 1] = id;
      palIn.value = parts.join("") + " ";
      palIn.focus();
      refreshPalette();
    };
    palSugg.appendChild(item);
  }

  function runPalette() {
    var line = palIn.value.trim();
    if (!line) return;
    execute(line).then(function (ok) {
      if (ok) {
        closePalette();
        toast(line);
      }
    });
  }

  function undo() {
    var before = {};
    snap.nodes.forEach(function (node) { before[node.id] = node.status; });
    backend
      .undo()
      .then(function () { return backend.snapshot(); })
      .then(function (next) {
        adopt(next);
        render();
        paintChrome();
        celebrate(before);
        toast(t("act.undo"));
      })
      .catch(function (error) { toast(message(error), true); });
  }

  /* ---------- toast ---------- */

  var toastTimer = null;
  function toast(text, bad) {
    toastEl.textContent = text;
    toastEl.className = "toast" + (bad ? " bad" : "");
    toastEl.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { toastEl.hidden = true; }, bad ? 6000 : 2600);
  }

  /* ---------- boot ---------- */

  function wire() {
    stage = document.getElementById("stage");
    canvas = document.getElementById("canvas");
    inspector = document.getElementById("inspector");
    dlg = document.getElementById("settings");
    palette = document.getElementById("palette");
    palIn = document.getElementById("pal");
    palOut = document.getElementById("palOut");
    palSugg = document.getElementById("palSugg");
    queueEl = document.getElementById("queue");
    queueBtn = document.getElementById("queueBtn");
    qInput = document.getElementById("q");
    searchWrap = document.getElementById("searchWrap");
    dockEl = document.getElementById("dock");
    rowEl = document.getElementById("row");
    toastEl = document.getElementById("toast");
    bannerEl = document.getElementById("banner");

    world = el("g");
    gDecor = el("g");
    gLinks = el("g");
    gFx = el("g");
    gNodes = el("g");
    [gDecor, gLinks, gFx, gNodes].forEach(function (g) { world.appendChild(g); });
    canvas.appendChild(world);

    wirePanZoom();

    qInput.addEventListener("input", function () {
      state.query = qInput.value;
      updateEmphasis();
    });
    document.getElementById("rxBtn").addEventListener("click", function () {
      state.regex = !state.regex;
      this.setAttribute("aria-pressed", String(state.regex));
      qInput.placeholder = state.regex ? t("search.phRx") : t("search.ph");
      updateEmphasis();
    });

    queueBtn.addEventListener("click", function () { toggleQueue(); });
    document.getElementById("qClose").addEventListener("click", function () {
      toggleQueue(false);
    });
    document.getElementById("allBtn").addEventListener("click", function () {
      state.focus = null;
      snap.areas.forEach(function (area) { state.visible[area.id] = true; });
      paintAreas();
      render();
      fit();
    });
    document.getElementById("dockToggle").addEventListener("click", function () {
      dockEl.hidden = !dockEl.hidden;
    });
    document.getElementById("setBtn").onclick = function () {
      paintSettings();
      dlg.showModal();
    };
    document.getElementById("setClose").onclick = function () { dlg.close(); };
    dlg.addEventListener("click", function (ev) {
      if (ev.target === dlg) dlg.close();
    });

    palIn.addEventListener("input", refreshPalette);
    palIn.addEventListener("keydown", function (ev) {
      if (ev.key === "Enter") { ev.preventDefault(); runPalette(); }
      else if (ev.key === "Escape") { ev.preventDefault(); closePalette(); }
      ev.stopPropagation();
    });
    palette.addEventListener("click", function (ev) {
      if (ev.target === palette) closePalette();
    });

    document.addEventListener("keydown", onKey);

    var resizeTimer = null;
    window.addEventListener("resize", function () {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(function () { fit(0); }, 140);
    });
  }

  function toggleQueue(on) {
    var show = on === undefined ? queueEl.hidden : on;
    queueEl.hidden = !show;
    queueBtn.setAttribute("aria-pressed", String(show));
    if (show) paintQueue();
  }

  function onKey(ev) {
    if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "k") {
      ev.preventDefault();
      if (palette.hidden) openPalette(); else closePalette();
      return;
    }
    if (dlg.open || !palette.hidden) return;
    var typing = ev.target === qInput;
    if (ev.key === ":" && !typing) { ev.preventDefault(); openPalette(); return; }
    if (ev.key === "/" && !typing) { ev.preventDefault(); qInput.focus(); qInput.select(); return; }
    if (ev.key === "Escape") {
      if (typing) { qInput.value = ""; state.query = ""; updateEmphasis(); qInput.blur(); }
      else if (!queueEl.hidden) toggleQueue(false);
      else select(null);
      return;
    }
    if (typing || ev.metaKey || ev.ctrlKey || ev.altKey) return;
    var key = ev.key.toLowerCase();
    if (key === "f") fit();
    else if (key === "q") toggleQueue();
    else if (key === "u") undo();
    else if (key === "s") { paintSettings(); dlg.showModal(); }
    else if (key === "1" || key === "2" || key === "3") {
      setLayout(LAYOUTS[Number(key) - 1], true);
    }
  }

  function boot() {
    loadPrefs();
    document.body.dataset.skin = state.skin;
    document.body.dataset.motion = state.motion;
    wire();
    rowEl.classList.toggle("dock-right", state.dockSide === "right");
    ensureFont(state.lang);
    if (window.innerWidth <= 860) dockEl.hidden = true;

    backend = B.connect();
    if (backend.readOnly) bannerEl.hidden = false;

    refresh()
      .then(function () {
        fit(0);
        var loading = document.getElementById("loading");
        if (loading) loading.remove();
      })
      .catch(function (error) {
        var loading = document.getElementById("loading");
        if (loading) loading.textContent = message(error);
      });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})(window);

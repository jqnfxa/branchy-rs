// Placing nodes on the canvas.
//
// Three projections of the same graph. Tier does the ordering in all three; the
// tier itself is computed by branchy-core and arrives in the snapshot, so this
// file only decides where things sit.
//
// A node with no tier is tangled in a cycle. It has no defined place in the
// ordering, so each layout parks such nodes together, past everything else.

(function (global) {
  "use strict";

  var R0 = 150;
  var RING = 118;
  var COL = 210;
  var ROW = 92;

  // Rows of nodes, ordered by tier, with cycle-tangled nodes last.
  function byTier(nodes) {
    var tiers = {};
    var tangled = [];
    nodes.forEach(function (node) {
      if (node.tier === null || node.tier === undefined) {
        tangled.push(node);
      } else {
        (tiers[node.tier] = tiers[node.tier] || []).push(node);
      }
    });
    var rows = Object.keys(tiers)
      .map(Number)
      .sort(function (a, b) {
        return a - b;
      })
      .map(function (t) {
        return tiers[t];
      });
    if (tangled.length) rows.push(tangled);
    return rows;
  }

  function prng(seed) {
    var h = 2166136261;
    for (var i = 0; i < seed.length; i++) {
      h ^= seed.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return function () {
      h += 0x6d2b79f5;
      var x = h;
      x = Math.imul(x ^ (x >>> 15), x | 1);
      x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
      return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
    };
  }

  // You in the middle, one angular sector per direction, one ring per tier.
  //
  // A direction gets only the rings it actually occupies, not one per absolute
  // tier: a direction whose lowest task is at tier 3 would otherwise sit alone
  // far out and force the whole disc to zoom out to fit it.
  function radial(areas, nodesByArea, jitter) {
    var positions = {};
    var sectors = [];
    var rings = 0;
    var total = 0;
    areas.forEach(function (area) {
      total += Math.max(1, (nodesByArea[area.id] || []).length);
    });
    if (!total) return { positions: positions, sectors: sectors, rings: rings };

    var gap = areas.length > 1 ? 0.13 : 0;
    var free = Math.PI * 2 - gap * areas.length;
    var angle = -Math.PI / 2;

    areas.forEach(function (area) {
      var list = nodesByArea[area.id] || [];
      var span = free * (Math.max(1, list.length) / total);
      var start = angle + gap / 2;
      var rowIndex = 0;

      byTier(list).forEach(function (row) {
        var index = rowIndex++;
        rings = Math.max(rings, index);
        var radius = R0 + index * RING;
        row.forEach(function (node, i) {
          var theta = start + ((i + 0.5) / row.length) * span;
          var r = radius;
          if (jitter) {
            var rnd = prng(node.id);
            theta += (rnd() - 0.5) * span * 0.34;
            r += (rnd() - 0.5) * RING * 0.62;
          }
          positions[node.id] = { x: Math.cos(theta) * r, y: Math.sin(theta) * r };
        });
      });

      sectors.push({
        area: area,
        mid: start + span / 2,
        r: R0 + (rowIndex - 0.35) * RING,
      });
      angle += span + gap;
    });

    return { positions: positions, sectors: sectors, rings: rings };
  }

  // Tiers left to right, one horizontal band per direction.
  function layered(areas, nodesByArea) {
    var positions = {};
    var sectors = [];
    var y = 0;

    areas.forEach(function (area) {
      var list = nodesByArea[area.id] || [];
      if (!list.length) return;
      var rows = byTier(list);
      var tallest = 1;
      rows.forEach(function (row) {
        tallest = Math.max(tallest, row.length);
      });
      rows.forEach(function (row, column) {
        var offset = (tallest - row.length) / 2;
        row.forEach(function (node, i) {
          positions[node.id] = { x: column * COL, y: y + (offset + i) * ROW };
        });
      });
      sectors.push({ area: area, band: { y0: y, y1: y + (tallest - 1) * ROW } });
      y += tallest * ROW + 78;
    });

    return { positions: positions, sectors: sectors, layered: true };
  }

  global.Branchy = global.Branchy || {};
  global.Branchy.layout = function (name, areas, nodes) {
    var nodesByArea = {};
    areas.forEach(function (area) {
      nodesByArea[area.id] = [];
    });
    nodes.forEach(function (node) {
      if (nodesByArea[node.area]) nodesByArea[node.area].push(node);
    });

    if (name === "layered") return layered(areas, nodesByArea);
    return radial(areas, nodesByArea, name === "web");
  };
  global.Branchy.layoutConstants = { R0: R0, RING: RING, COL: COL, ROW: ROW };
})(window);

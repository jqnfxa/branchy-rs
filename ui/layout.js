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

  // A real project backlog is mostly flat: a hundred items with no
  // prerequisites between them all land in tier 0. Put on one ring, or in one
  // column, they smear into a solid band. So a ring holds only as many nodes
  // as fit MIN_ARC apart, and the rest wrap onto further rings WRAP apart,
  // closer than tiers are, so a wrapped tier still reads as one tier. A graph
  // where nothing wraps is laid out exactly as it was before.
  var MIN_ARC = 46;
  var WRAP = 64;

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
  // How many nodes each ring of a wrapped tier takes, starting at `radius`.
  // Shares follow each ring's capacity, so the outer rings, which are
  // longer, take more and the spacing stays even.
  function wrapRings(count, span, radius) {
    var caps = [];
    var room = 0;
    while (room < count) {
      var cap = Math.max(1, Math.floor((span * (radius + caps.length * WRAP)) / MIN_ARC));
      caps.push(cap);
      room += cap;
    }
    var shares = [];
    var given = 0;
    caps.forEach(function (cap, i) {
      var share = i === caps.length - 1 ? count - given : Math.round((count * cap) / room);
      shares.push(share);
      given += share;
    });
    return shares;
  }

  function radial(areas, nodesByArea, jitter) {
    var positions = {};
    var sectors = [];
    var rings = 0;
    var outermost = R0;
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
      var radius = R0 - RING;

      byTier(list).forEach(function (row) {
        radius += RING;
        var shares = wrapRings(row.length, span, radius);
        var wrapped = shares.length > 1;
        var at = 0;
        shares.forEach(function (share, ringIndex) {
          if (ringIndex > 0) radius += WRAP;
          var step = span / share;
          row.slice(at, at + share).forEach(function (node, i) {
            var theta = start + (i + 0.5) * step;
            var r = radius;
            if (jitter) {
              var rnd = prng(node.id);
              // on a wrapped ring, bounded by the spacing, or the crowd
              // would scatter across itself
              theta += (rnd() - 0.5) * (wrapped ? Math.min(span * 0.34, step * 1.1) : span * 0.34);
              r += (rnd() - 0.5) * (wrapped ? WRAP : RING) * 0.62;
            }
            positions[node.id] = { x: Math.cos(theta) * r, y: Math.sin(theta) * r };
          });
          at += share;
        });
      });

      outermost = Math.max(outermost, radius);
      sectors.push({
        area: area,
        mid: start + span / 2,
        r: radius + RING * 0.65,
      });
      angle += span + gap;
    });

    // the decorative rings stay evenly spaced; with nothing wrapped, every
    // node sits on one of them
    rings = Math.ceil((outermost - R0) / RING);
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
      // a tier taller than this wraps into several columns: roughly as many
      // rows as make the band square, given how wide a column is
      var most = Math.max(14, Math.ceil(Math.sqrt((list.length * COL) / ROW)));
      var columns = [];
      byTier(list).forEach(function (row) {
        for (var at = 0; at < row.length; at += most) {
          columns.push({ nodes: row.slice(at, at + most), wraps: row.length > most });
        }
        // a little air after a wrapped tier, so its columns read as one
        if (row.length > most) columns.push(null);
      });
      var tallest = 1;
      columns.forEach(function (column) {
        if (column) tallest = Math.max(tallest, column.nodes.length);
      });
      var x = 0;
      columns.forEach(function (column) {
        if (!column) {
          x += COL * 0.3;
          return;
        }
        var offset = column.wraps ? 0 : (tallest - column.nodes.length) / 2;
        column.nodes.forEach(function (node, i) {
          positions[node.id] = { x: x, y: y + (offset + i) * ROW };
        });
        x += COL;
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

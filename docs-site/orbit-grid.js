// Box-grid glow: cells near the cursor light up in mint and fade out, and a few cells
// pulse on their own. Drawn on a fixed canvas behind the page; off for reduced motion.
(function () {
  if (typeof window === "undefined" || window.__orbitGrid) return;
  window.__orbitGrid = true;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

  var CELL = 46;
  var RADIUS = 10;
  var canvas, ctx, dpr, w, h;
  var cells = new Map(); // "col,row" -> intensity 0..1
  var last = -1;

  function setup() {
    canvas = document.getElementById("orbit-grid-canvas");
    if (!canvas) {
      canvas = document.createElement("canvas");
      canvas.id = "orbit-grid-canvas";
      canvas.setAttribute("aria-hidden", "true");
      document.body.prepend(canvas);
    }
    ctx = canvas.getContext("2d");
    resize();
  }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = window.innerWidth;
    h = window.innerHeight;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    canvas.style.width = w + "px";
    canvas.style.height = h + "px";
  }

  function light(col, row, v) {
    var k = col + "," + row;
    cells.set(k, Math.max(cells.get(k) || 0, v));
  }

  function onMove(e) {
    var col = Math.floor(e.clientX / CELL);
    var row = Math.floor(e.clientY / CELL);
    var k = col * 100000 + row;
    if (k === last) return;
    last = k;
    light(col, row, 1);
    light(col + 1, row, 0.35);
    light(col - 1, row, 0.35);
    light(col, row + 1, 0.35);
    light(col, row - 1, 0.35);
  }

  function roundRect(x, y, s, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + s, y, x + s, y + s, r);
    ctx.arcTo(x + s, y + s, x, y + s, r);
    ctx.arcTo(x, y + s, x, y, r);
    ctx.arcTo(x, y, x + s, y, r);
    ctx.closePath();
  }

  function frame() {
    // A random cell in the top band pulses now and then.
    if (Math.random() < 0.03) {
      light(Math.floor(Math.random() * (w / CELL)), Math.floor(Math.random() * Math.min(10, h / CELL)), 0.6);
    }
    var dark = document.documentElement.classList.contains("dark");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    cells.forEach(function (v, k) {
      var p = k.split(",");
      var x = +p[0] * CELL + 0.5;
      var y = +p[1] * CELL + 0.5;
      roundRect(x, y, CELL - 1, RADIUS);
      ctx.fillStyle = "rgba(127, 250, 183, " + (dark ? 0.16 : 0.35) * v + ")";
      ctx.fill();
      ctx.strokeStyle = "rgba(15, 157, 88, " + (dark ? 0.5 : 0.35) * v + ")";
      ctx.lineWidth = 0.8;
      ctx.stroke();
      var next = v - 0.018;
      if (next <= 0) cells.delete(k);
      else cells.set(k, next);
    });
    requestAnimationFrame(frame);
  }

  function start() {
    setup();
    window.addEventListener("resize", resize);
    window.addEventListener("pointermove", onMove, { passive: true });
    requestAnimationFrame(frame);
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();

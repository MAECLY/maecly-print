/* What moves, once, and only towards the finished page the HTML already
   shows. Any error removes the motion classes and leaves that page.
   Also: the third-party request counter in the footer. */
(function () {
  "use strict";

  var html = document.documentElement;
  var stops = [];

  /* Off-origin http(s) requests seen by resource timing. */
  function counter() {
    var out = document.querySelector(".req-count"), ext = document.querySelector(".proof-ext");
    if (!out || !window.PerformanceObserver || !performance.getEntriesByType) return;
    var seen = {}, n = 0;
    var look = function (list) {
      list.forEach(function (e) {
        try {
          var u = new URL(e.name);
          if (/^https?:$/.test(u.protocol) && u.origin !== location.origin && !seen[e.name]) seen[e.name] = ++n;
        } catch (x) {}
      });
      out.textContent = n;
      if (ext) ext.hidden = n === 0;
    };
    look(performance.getEntriesByType("resource"));
    try {
      new PerformanceObserver(function (l) { look(l.getEntries()); }).observe({ type: "resource", buffered: true });
    } catch (x) {}
  }

  function onScreen(el) {
    var r = el.getBoundingClientRect();
    return r.width > 0 && r.bottom > 0 && r.top < innerHeight;
  }

  function once(target, threshold, fn) {
    var io = new IntersectionObserver(function (entries) {
      if (!entries.some(function (e) { return e.isIntersecting; })) return;
      io.disconnect();
      fn();
    }, { threshold: threshold });
    io.observe(target);
  }

  function timers() {
    var list = [];
    var t = function (fn, ms, stop) {
      list.push(setTimeout(function () {
        try { fn(); } catch (x) { stop(); }
      }, ms));
    };
    t.clear = function () { list.forEach(clearTimeout); list = []; };
    return t;
  }

  /* Hero: pick «Print with Pluryprint», the printer fans sheets out in three
     directions while the queue counts to its total, then the three resting
     sheets settle into the icon's fan. */
  function hero() {
    var press = document.querySelector(".press");
    if (!press) return;
    var fig = press.closest("figure");
    var count = press.querySelector(".count"), fill = press.querySelector(".counter"), state = press.querySelector(".counter-state");
    if (!count || !fill || !state) return;
    var late = performance.now() > 2600;
    var total = +count.textContent, done = state.textContent, busy = state.getAttribute("data-busy") || done;
    var queued = state.getAttribute("data-queued") || busy;
    var later = timers(), raf = 0;
    var phases = ["armed", "picked", "printing"];
    var reset = function () {
      later.clear();
      cancelAnimationFrame(raf);
      press.classList.remove.apply(press.classList, phases);
    };
    var stop = function () {
      reset();
      count.textContent = total;
      fill.style.setProperty("--p", 1);
      state.textContent = done;
      if (fig) fig.classList.add("is-played");
    };
    stops.push(stop);
    var tick = function (ms) {
      var t0 = performance.now();
      var step = function (now) {
        var k = Math.min(1, (now - t0) / ms);
        var e = 1 - Math.pow(1 - k, 2.2);
        count.textContent = Math.round(e * total);
        fill.style.setProperty("--p", e.toFixed(3));
        if (k < 1) raf = requestAnimationFrame(step);
      };
      raf = requestAnimationFrame(step);
    };
    var play = function () {
      reset();
      if (fig) fig.classList.remove("is-played");
      press.classList.add("armed");
      count.textContent = 0;
      fill.style.setProperty("--p", 0);
      state.textContent = busy;
      later(function () { press.classList.add("picked"); }, 350, stop);
      later(function () { press.classList.add("printing"); tick(3000); }, 950, stop);
      later(function () {
        press.classList.remove("printing", "armed");
        state.textContent = done;
      }, 4050, stop);
      later(function () { press.classList.remove("picked"); if (fig) fig.classList.add("is-played"); }, 5000, stop);
    };
    if (late) {
      if (fig) fig.classList.add("is-played");
    } else {
      press.classList.add("armed");
      count.textContent = 0;
      fill.style.setProperty("--p", 0);
      state.textContent = queued;
      once(press, 0.4, function () { later(play, 250, stop); });
    }
    var replay = fig && fig.querySelector(".replay");
    if (replay) replay.addEventListener("click", function () {
      try { play(); } catch (x) { stop(); }
    });
  }

  /* The terminal types its command, then shows the JSON. */
  function terminal() {
    var cli = document.querySelector(".cli");
    var cmd = cli && cli.querySelector(".t-cmd");
    if (!cmd) return;
    var text = cmd.textContent, later = timers();
    var caret = document.createElement("span");
    caret.className = "t-caret";
    var stop = function () {
      later.clear();
      cmd.textContent = text;
      cli.classList.remove("typing");
      if (caret.parentNode) caret.parentNode.removeChild(caret);
    };
    stops.push(stop);
    if (onScreen(cli)) return;
    cli.classList.add("typing");
    cmd.textContent = "";
    cmd.parentNode.insertBefore(caret, cmd.nextSibling);
    var type = function (i) {
      if (i > text.length) {
        later(function () { cli.classList.remove("typing"); }, 260, stop);
        later(function () { if (caret.parentNode) caret.parentNode.removeChild(caret); }, 1400, stop);
        return;
      }
      cmd.textContent = text.slice(0, i);
      later(function () { type(i + 1); }, 22 + ((i * 37) % 17) + (text.charAt(i) === " " ? 30 : 0), stop);
    };
    once(cli, 0.45, function () { later(function () { type(1); }, 300, stop); });
  }

  /* Already on screen at load: shown, not played. */
  function reveals() {
    var items = document.querySelectorAll("[data-reveal]");
    for (var i = 0; i < items.length; i++) {
      var el = items[i];
      if (onScreen(el)) el.classList.add("is-in", "no-anim");
      else once(el, 0.35, el.classList.add.bind(el.classList, "is-in"));
    }
  }

  function halt() {
    stops.forEach(function (s) { try { s(); } catch (x) {} });
    html.classList.remove("ink", "motion");
  }

  counter();
  if (!html.classList.contains("ink") || !window.IntersectionObserver || !window.requestAnimationFrame) return html.classList.remove("ink");
  try {
    hero();
    terminal();
    reveals();
    html.classList.add("motion");
  } catch (x) {
    halt();
  }

  /* Motion turned off mid-visit. */
  var still = matchMedia("(prefers-reduced-motion: reduce)");
  var off = function () { if (still.matches) halt(); };
  if (still.addEventListener) still.addEventListener("change", off);
  else if (still.addListener) still.addListener(off);
})();

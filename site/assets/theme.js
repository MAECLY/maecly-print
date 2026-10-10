/* Runs in <head>, before first paint: colour mode, motion gate, OS hint.
   No-JS visitors get the system mode through CSS alone. */
(function () {
  "use strict";
  var html = document.documentElement;
  var KEY = "pluryprint:mode";
  var BG = { light: "#faf9f6", dark: "#15140f" };
  var dark = window.matchMedia ? matchMedia("(prefers-color-scheme: dark)") : null;

  html.classList.add("js");

  function read() {
    try {
      var m = localStorage.getItem(KEY);
      return m === "light" || m === "dark" ? m : "system";
    } catch (e) {
      return "system";
    }
  }
  function resolve(mode) {
    return mode === "system" ? (dark && dark.matches ? "dark" : "light") : mode;
  }
  /* A <picture> follows prefers-color-scheme by itself; a forced mode
     re-points its dark <source> so the screenshots match the page. */
  function pictures(mode, theme) {
    var list = document.querySelectorAll("source[data-dark]");
    for (var i = 0; i < list.length; i++) {
      list[i].media = mode === "system" ? "(prefers-color-scheme: dark)" : theme === "dark" ? "all" : "not all";
    }
  }
  function apply(mode) {
    var theme = resolve(mode);
    html.setAttribute("data-theme", theme);
    html.setAttribute("data-mode", mode);
    var metas = document.querySelectorAll('meta[name="theme-color"]');
    for (var i = 0; i < metas.length; i++) {
      metas[i].content = mode === "system" ? (metas[i].media.indexOf("dark") > -1 ? BG.dark : BG.light) : BG[theme];
    }
    pictures(mode, theme);
    var radios = document.querySelectorAll('input[name="mode"]');
    for (var j = 0; j < radios.length; j++) radios[j].checked = radios[j].value === mode;
  }

  var mode = read();
  apply(mode);

  /* Motion only when nobody asked for less of it. */
  var still = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
  var save = navigator.connection && navigator.connection.saveData;
  if (!still && !save) html.classList.add("ink");

  var ua = navigator.userAgent || "";
  var os = /iPhone|iPad|iPod/.test(ua) ? "ios" : /Android/.test(ua) ? "android" : /Mac/.test(ua) ? "mac" : /Win/.test(ua) ? "win" : /Linux/.test(ua) ? "linux" : "other";
  html.setAttribute("data-os", os);

  function follow() { if (mode === "system") apply(mode); }
  if (dark) {
    if (dark.addEventListener) dark.addEventListener("change", follow);
    else if (dark.addListener) dark.addListener(follow);
  }
  window.addEventListener("storage", function (e) {
    if (e.key === KEY) { mode = read(); apply(mode); }
  });

  function choose(next, x, y) {
    if (next === mode) return;
    var update = function () {
      mode = next;
      try {
        if (next === "system") localStorage.removeItem(KEY);
        else localStorage.setItem(KEY, next);
      } catch (e) {}
      apply(next);
    };
    var calm = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!document.startViewTransition || calm || resolve(next) === html.getAttribute("data-theme")) return update();
    html.style.setProperty("--ink-x", x + "px");
    html.style.setProperty("--ink-y", y + "px");
    html.classList.add("vt-ink");
    var end = function () { html.classList.remove("vt-ink"); };
    try {
      document.startViewTransition(update).finished.then(end, end);
    } catch (e) {
      end();
      update();
    }
  }

  document.addEventListener("DOMContentLoaded", function () {
    apply(mode);
    var lastX = innerWidth / 2, lastY = 0;
    document.addEventListener("pointerdown", function (e) { lastX = e.clientX; lastY = e.clientY; }, true);
    /* The phone section menu closes once a link is chosen. */
    var menu = document.querySelector(".navmenu");
    if (menu) menu.addEventListener("click", function (e) {
      if (e.target.closest("a")) menu.open = false;
    });
    var radios = document.querySelectorAll('input[name="mode"]');
    for (var i = 0; i < radios.length; i++) {
      radios[i].addEventListener("change", function (e) {
        var r = e.target.closest("label").getBoundingClientRect();
        var fromKey = e.target.matches(":focus-visible");
        choose(e.target.value, fromKey ? r.left + r.width / 2 : lastX, fromKey ? r.top + r.height / 2 : lastY);
      });
    }
  });
})();

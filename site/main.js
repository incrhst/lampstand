// The Order of Worship goes live: a LIVE marker moves down the service and
// the projector screen cuts to that item's slide, like an operator at the desk.
(() => {
  const order = document.getElementById("order");
  const items = [...document.querySelectorAll(".service-item")];
  const slides = new Map([...document.querySelectorAll(".slide")].map((s) => [s.dataset.for, s]));
  const liveName = document.getElementById("live-name");
  const pending = document.querySelector(".s-pending");
  const chip = document.querySelector(".s-chip");
  const count = document.querySelector(".s-count");
  const booth = document.querySelector(".booth");
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  const narrow = matchMedia("(max-width: 880px)");
  if (!order || !items.length) return;

  let live = -1;
  let built = false;
  // Follow the scroll only once the visitor has scrolled, so the page opens on the first item.
  let quietUntil = Infinity;
  addEventListener("scroll", () => { if (quietUntil === Infinity) quietUntil = 0; }, { once: true, passive: true });
  let timer = 0;
  let buildTimer = 0;

  function setBuilt(on) {
    built = on;
    pending.classList.toggle("is-built", on);
    chip.textContent = on ? "Build 3 of 3" : "Build 2 of 3";
  }

  function countdown(on) {
    clearInterval(timer);
    if (!on) return;
    let left = 120;
    const show = () => { count.textContent = `${String(Math.floor(left / 60)).padStart(2, "0")}:${String(left % 60).padStart(2, "0")}`; };
    show();
    timer = setInterval(() => { left = left > 0 ? left - 1 : 120; show(); }, 1000);
  }

  function goLive(i) {
    i = Math.max(0, Math.min(items.length - 1, i));
    if (i === live) return;
    live = i;
    const key = items[i].dataset.slide;
    items.forEach((el, n) => el.setAttribute("aria-current", n === i ? "true" : "false"));
    slides.forEach((s, k) => s.classList.toggle("is-live", k === key));
    liveName.textContent = items[i].querySelector(".item-name").textContent;
    setBuilt(false);
    countdown(key === "welcome");
    // A visitor who only scrolls still sees the deck build, one step after a beat.
    clearTimeout(buildTimer);
    if (key === "sermon" && !calm.matches) buildTimer = setTimeout(() => setBuilt(true), 1200);
  }

  // Next steps through a deck's builds before moving on, one click at a time.
  function step(dir, fromKey) {
    quietUntil = Date.now() + 900;
    clearTimeout(buildTimer);
    if (dir > 0 && items[live]?.dataset.slide === "sermon" && !built) return setBuilt(true);
    if (dir < 0 && items[live]?.dataset.slide === "sermon" && built) return setBuilt(false);
    goLive(live + dir);
    if (fromKey) items[live].scrollIntoView({ block: "nearest", behavior: calm.matches ? "auto" : "smooth" });
  }

  items.forEach((el, i) => el.addEventListener("click", () => { quietUntil = Date.now() + 900; goLive(i); }));
  document.querySelectorAll("[data-step]").forEach((b) => b.addEventListener("click", () => step(Number(b.dataset.step), true)));

  // Left and right step through the service; page scrolling keys stay with the page.
  order.addEventListener("keydown", (e) => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.key === "ArrowRight") { e.preventDefault(); step(1, true); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); step(-1, true); }
  });

  // Scrolling the order of worship moves the live marker with you.
  if ("IntersectionObserver" in window) {
    let io;
    const watch = () => {
      io?.disconnect();
      // On a phone the screen sticks to the top, so follow the item just below it.
      const boothH = narrow.matches ? Math.round(booth.getBoundingClientRect().height) : 0;
      order.style.setProperty("--booth-h", `${boothH + 12}px`);
      const top = narrow.matches ? boothH + 24 : Math.round(innerHeight * 0.45);
      const bottom = Math.max(0, innerHeight - top - Math.round(innerHeight * 0.1));
      io = new IntersectionObserver((entries) => {
        if (Date.now() < quietUntil) return;
        const hit = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (hit) goLive(items.indexOf(hit.target));
      }, { rootMargin: `-${top}px 0px -${bottom}px 0px` });
      items.forEach((el) => io.observe(el));
    };
    let resizeTimer = 0;
    addEventListener("resize", () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(watch, 150); });
    narrow.addEventListener("change", watch);
    watch();
  }

  goLive(0);
})();

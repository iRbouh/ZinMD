// ZinMD's page: the few things that need a script. The page reads and works
// without it; this adds the motion that follows the pointer, the name of the
// reader's system on the download button, and links straight to the files of
// the latest release.

const REPO = "iRbouh/ZinMD";
const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ---- which system is this?
function system() {
  const hint = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  const agent = navigator.userAgent || "";
  if (/android|iphone|ipad|ipod/i.test(agent)) return null;
  if (/mac/i.test(hint)) return "mac";
  if (/win/i.test(hint)) return "windows";
  if (/linux|x11/i.test(hint)) return "linux";
  return null;
}

const os = system();
const names = { mac: "Mac", windows: "Windows", linux: "Linux" };
if (os) {
  const label = document.querySelector("[data-os-label]");
  if (label) label.textContent = "Download for " + names[os];
  const card = document.querySelector('.sys[data-os="' + os + '"]');
  if (card) card.classList.add("is-yours");
}

// ---- the files of the latest release. Until GitHub answers, and if it does
// not, every button leads to the release's own page.
fetch("https://api.github.com/repos/" + REPO + "/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
  .then((answer) => (answer.ok ? answer.json() : Promise.reject(answer.status)))
  .then((release) => {
    const version = String(release.tag_name || "").replace(/^v/, "");
    if (version) {
      document.querySelectorAll("[data-version]").forEach((place) => (place.textContent = version));
    }
    const files = release.assets || [];
    document.querySelectorAll("[data-asset]").forEach((link) => {
      const file = files.find((asset) => asset.name.endsWith(link.dataset.asset));
      if (file) {
        link.href = file.browser_download_url;
        link.title = file.name + ", " + Math.round(file.size / 1048576) + " MB";
      }
    });
  })
  .catch(() => {});

// ---- the menu is a little more solid once the page has moved under it
const nav = document.querySelector(".nav");
const settle = () => nav.classList.toggle("scrolled", window.scrollY > 24);
settle();
window.addEventListener("scroll", settle, { passive: true });

// ---- the scene comes in, layer by layer
const stage = document.querySelector(".stage");
requestAnimationFrame(() => requestAnimationFrame(() => stage.classList.add("in")));

// ---- the scene leans after the pointer: near layers go further than far ones
if (!still && window.matchMedia("(hover: hover) and (pointer: fine)").matches) {
  const hero = document.querySelector(".hero");
  let aim = [0, 0];
  let now = [0, 0];
  let moving = false;

  const step = () => {
    now[0] += (aim[0] - now[0]) * 0.07;
    now[1] += (aim[1] - now[1]) * 0.07;
    stage.style.setProperty("--mx", now[0].toFixed(4));
    stage.style.setProperty("--my", now[1].toFixed(4));
    if (Math.abs(aim[0] - now[0]) + Math.abs(aim[1] - now[1]) > 0.001) {
      requestAnimationFrame(step);
    } else {
      moving = false;
    }
  };
  const lean = (x, y) => {
    aim = [x, y];
    if (!moving) {
      moving = true;
      requestAnimationFrame(step);
    }
  };

  hero.addEventListener("pointermove", (event) => {
    const box = hero.getBoundingClientRect();
    lean((event.clientX - box.left) / box.width - 0.5, (event.clientY - box.top) / box.height - 0.5);
  });
  hero.addEventListener("pointerleave", () => lean(0, 0));
}

// ---- the line for Terminal can be copied with a click
document.querySelectorAll("[data-copy]").forEach((button) => {
  if (!navigator.clipboard) return;
  button.hidden = false;
  button.addEventListener("click", () => {
    navigator.clipboard.writeText(button.dataset.copy).then(() => {
      button.textContent = "Copied";
      button.classList.add("done");
      setTimeout(() => {
        button.textContent = "Copy";
        button.classList.remove("done");
      }, 1800);
    });
  });
});

// ---- the rest of the page comes in as it is scrolled to
const hidden = document.querySelectorAll(".reveal");
if (still || !("IntersectionObserver" in window)) {
  hidden.forEach((thing) => thing.classList.add("in"));
} else {
  const watch = new IntersectionObserver(
    (seen) => {
      seen.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("in");
          watch.unobserve(entry.target);
        }
      });
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.08 }
  );
  hidden.forEach((thing, place) => {
    // Cards in a row come in one after the other.
    thing.style.transitionDelay = (place % 3) * 80 + "ms";
    watch.observe(thing);
  });
}

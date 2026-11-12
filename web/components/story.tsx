"use client";
import { useEffect, useRef, useState } from "react";
import * as THREE from "three";

// The mechanism, told with the money itself. Forty USDC coins are one deposit. Six become white thinking-credit chips,
// the rest go to the agent's Hyperliquid account. Two chips are spent on model calls (each leaves a signed receipt),
// the unspent four flip back into USDC and pass through NEAR Intents into the account, and the NFT ends up owning it all.
type V3 = [number, number, number];

const CHAPTERS: { label: string; title: string; body: string; tags: { text: string; at: V3 }[] }[] = [
  {
    label: "01 Deposit",
    title: "One deposit, held one to one",
    body: "You fund an agent once, in USDC. The contract keeps it as a reserve that always covers every agent's credits and margin.",
    tags: [
      { text: "20 USDC", at: [2.75, 1.4, 0] },
    ],
  },
  {
    label: "02 Split",
    title: "You choose how much it may think",
    body: "A slider divides the deposit. White credits pay for model calls. The USDC goes straight to the agent's own Hyperliquid account, so it can trade from the first hour.",
    tags: [
      { text: "thinking credits, 15%", at: [-2.1, 2.3, 0] },
      { text: "trading margin, 85%", at: [3.05, 2.05, 0] },
    ],
  },
  {
    label: "03 Think and trade",
    title: "Every decision is paid for and signed",
    body: "Each cycle the agent asks its model what to do. One credit pays for the call, the trade goes out with a trade-only key, and the model's signed receipt is stored on NEAR.",
    tags: [
      { text: "one credit per model call", at: [-2.1, 2.3, 0] },
      { text: "live trades", at: [3.05, 1.55, 0] },
    ],
  },
  {
    label: "04 Convert",
    title: "What it does not spend becomes margin",
    body: "When the epoch ends, the credits it did not use turn back into USDC and move through NEAR Intents into its trading account. Think less, trade bigger.",
    tags: [
      { text: "NEAR Intents", at: [-1.95, 1.55, 0] },
      { text: "unused credits become margin", at: [3.05, -2.0, 0] },
    ],
  },
  {
    label: "05 Own",
    title: "The NFT is the agent",
    body: "The Hyperliquid account belongs to the NFT through NEAR Chain Signatures. Sell it and the account, the margin and the track record go with it, at a live price.",
    tags: [{ text: "one NFT: account, margin, record", at: [1.85, 2.1, 0] }],
  },
];

const N = 40;
const CREDITS = 6;
const SPENT = 2;
const CARD = { x: 1.05, y: 0.05, w: 3.95, h: 4.6 };
const NFT = { x: 0.0, y: 0.1, w: 3.3, h: 4.5 };
const RING: V3 = [-1.5, 0.75, 0.8];
const MODEL: V3 = [-2.55, -1.0, 0];

type State = { pos: V3; scale: number; usdc: boolean };

function layout(): State[][] {
  const L: State[][] = Array.from({ length: 5 }, () => []);
  const grid = (k: number, cols: number, x0: number, y0: number, gap: number): V3 => [x0 + (k % cols) * gap, y0 - Math.floor(k / cols) * gap, 0.05];
  for (let i = 0; i < N; i++) {
    const credit = i < CREDITS;
    const k = i - CREDITS;
    L[0].push({ pos: [-2.45 + (i % 8) * 0.7, 1.35 - Math.floor(i / 8) * 0.68, 0], scale: 1, usdc: true });
    L[1].push(credit ? { pos: [-2.55, 1.75 - i * 0.6, 0.2], scale: 0.95, usdc: false } : { pos: grid(k, 6, CARD.x - 1.4, 1.2, 0.56), scale: 0.8, usdc: true });
    L[2].push(
      credit
        ? i < SPENT
          ? { pos: MODEL, scale: 0, usdc: false }
          : { pos: [-2.55, 1.75 - (i - SPENT) * 0.6, 0.2], scale: 0.95, usdc: false }
        : { pos: grid(k, 9, CARD.x - 1.6, -0.45, 0.4), scale: 0.56, usdc: true },
    );
    L[3].push(
      credit
        ? i < SPENT
          ? { pos: MODEL, scale: 0, usdc: false }
          : { pos: grid(N - CREDITS + (i - SPENT), 9, CARD.x - 1.6, -0.45, 0.4), scale: 0.56, usdc: true }
        : { pos: grid(k, 9, CARD.x - 1.6, -0.45, 0.4), scale: 0.56, usdc: true },
    );
    const j = credit ? (i < SPENT ? -1 : N - CREDITS + (i - SPENT)) : k;
    L[4].push(j < 0 ? { pos: MODEL, scale: 0, usdc: false } : { pos: grid(j, 8, NFT.x - 1.19, -0.15, 0.34), scale: 0.42, usdc: true });
  }
  return L;
}

const fontVar = (name: string, fallback: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;

function canvasTexture(w: number, h: number, draw: (c: CanvasRenderingContext2D, W: number, H: number) => void) {
  const scale = 256;
  const cv = document.createElement("canvas");
  cv.width = Math.round(w * scale);
  cv.height = Math.round(h * scale);
  const c = cv.getContext("2d")!;
  draw(c, cv.width, cv.height);
  const tex = new THREE.CanvasTexture(cv);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.anisotropy = 4;
  return tex;
}

function usdcFace(sans: string) {
  return canvasTexture(2, 2, (c, W) => {
    const r = W / 2;
    c.fillStyle = "#2775ca";
    c.beginPath(); c.arc(r, r, r, 0, Math.PI * 2); c.fill();
    c.strokeStyle = "#ffffff"; c.lineWidth = W * 0.05; c.lineCap = "round";
    c.beginPath(); c.arc(r, r, r * 0.7, Math.PI * 0.64, Math.PI * 1.36); c.stroke();
    c.beginPath(); c.arc(r, r, r * 0.7, -Math.PI * 0.36, Math.PI * 0.36); c.stroke();
    c.fillStyle = "#ffffff"; c.textAlign = "center"; c.textBaseline = "middle";
    c.font = `600 ${W * 0.46}px ${sans}`;
    c.fillText("$", r, r + W * 0.03);
  });
}

function creditFace(mono: string) {
  return canvasTexture(2, 2, (c, W) => {
    const r = W / 2;
    c.fillStyle = "#eef2ff";
    c.beginPath(); c.arc(r, r, r, 0, Math.PI * 2); c.fill();
    c.strokeStyle = "#1c2a6b"; c.lineWidth = W * 0.03;
    c.beginPath(); c.arc(r, r, r * 0.78, 0, Math.PI * 2); c.stroke();
    c.fillStyle = "#1c2a6b"; c.textAlign = "center"; c.textBaseline = "middle";
    c.font = `500 ${W * 0.15}px ${mono}`;
    c.fillText("credit", r, r);
  });
}

function panel(w: number, h: number, paint: (c: CanvasRenderingContext2D, W: number, H: number) => void) {
  const tex = canvasTexture(w, h, (c, W, H) => {
    c.clearRect(0, 0, W, H);
    paint(c, W, H);
  });
  return new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ map: tex, transparent: true, depthWrite: false }));
}

function frame(c: CanvasRenderingContext2D, W: number, H: number, fill: string | CanvasGradient, stroke: string, radius = 0.09) {
  const r = Math.min(W, H) * radius;
  c.beginPath(); c.roundRect(4, 4, W - 8, H - 8, r);
  c.fillStyle = fill; c.fill();
  c.lineWidth = 4; c.strokeStyle = stroke; c.stroke();
}

const smooth = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

export function Story() {
  const section = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const tags = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);

  useEffect(() => {
    const el = section.current, cv = canvas.current, tagBox = tags.current;
    if (!el || !cv || !tagBox) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas: cv, antialias: true, alpha: true });
    } catch {
      return;
    }
    let disposed = false;
    let raf = 0;
    const cleanup: (() => void)[] = [];

    document.fonts.ready.then(() => {
      if (disposed) return;
      const sans = fontVar("--font-sans", "sans-serif");
      const mono = fontVar("--font-mono", "monospace");
      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 60);
      scene.add(new THREE.AmbientLight(0xffffff, 1.1));
      const key = new THREE.DirectionalLight(0xdfe8ff, 2.2);
      key.position.set(-3, 4, 6);
      scene.add(key);
      const world = new THREE.Group();
      scene.add(world);

      // Coins: one cylinder, faces carry the mark, the rim is milled metal.
      const coinGeo = new THREE.CylinderGeometry(0.3, 0.3, 0.07, 48, 1);
      coinGeo.rotateX(Math.PI / 2);
      const usdcTex = usdcFace(sans);
      const creditTex = creditFace(mono);
      for (const t of [usdcTex, creditTex]) { t.center.set(0.5, 0.5); t.rotation = Math.PI / 2; }
      const rimUsdc = new THREE.MeshStandardMaterial({ color: 0x9fb8e8, metalness: 0.7, roughness: 0.32 });
      const rimCredit = new THREE.MeshStandardMaterial({ color: 0xdfe5f7, metalness: 0.3, roughness: 0.4 });
      const usdc = new THREE.InstancedMesh(coinGeo, [rimUsdc, new THREE.MeshBasicMaterial({ map: usdcTex }), new THREE.MeshBasicMaterial({ map: usdcTex })], N);
      const credit = new THREE.InstancedMesh(coinGeo, [rimCredit, new THREE.MeshBasicMaterial({ map: creditTex }), new THREE.MeshBasicMaterial({ map: creditTex })], N);
      world.add(usdc, credit);

      // Places the money moves between.
      const reserve = panel(5.7, 0.62, (c, W, H) => {
        frame(c, W, H, "rgba(16,23,61,0.9)", "rgba(190,205,255,0.22)", 0.3);
        c.fillStyle = "#aab4d8"; c.font = `400 ${H * 0.26}px ${mono}`; c.textBaseline = "middle"; c.textAlign = "center";
        c.fillText("NEAR contract  ·  USDC held 1:1 as reserve", W / 2, H / 2);
      });
      reserve.position.set(0, -2.15, -0.2);

      const card = panel(CARD.w, CARD.h, (c, W, H) => {
        const g = c.createLinearGradient(0, 0, 0, H);
        g.addColorStop(0, "#14205a"); g.addColorStop(1, "#0b1236");
        frame(c, W, H, g, "rgba(190,205,255,0.28)", 0.06);
        c.fillStyle = "#f3f5ff"; c.font = `400 ${W * 0.058}px ${sans}`; c.textBaseline = "top";
        c.fillText("Hyperliquid account", W * 0.07, H * 0.045);
        c.fillStyle = "#7682ad"; c.font = `400 ${W * 0.034}px ${mono}`;
        c.fillText("owned by the NFT via NEAR Chain Signatures", W * 0.07, H * 0.11);
      });
      card.position.set(CARD.x, CARD.y, -0.25);

      const model = panel(1.7, 0.66, (c, W, H) => {
        frame(c, W, H, "#10173d", "rgba(156,194,255,0.6)", 0.22);
        c.fillStyle = "#f3f5ff"; c.textAlign = "center"; c.textBaseline = "middle";
        c.font = `400 ${H * 0.22}px ${mono}`; c.fillText("NEAR AI model call", W / 2, H * 0.38);
        c.fillStyle = "#7682ad"; c.font = `400 ${H * 0.17}px ${mono}`; c.fillText("signed in a TEE", W / 2, H * 0.66);
      });
      model.position.set(MODEL[0], MODEL[1], -0.1);

      const slips = [0, 1].map((j) => {
        const s = panel(1.7, 0.4, (c, W, H) => {
          frame(c, W, H, "#eef2ff", "rgba(0,0,0,0)", 0.14);
          c.fillStyle = "#1c2a6b"; c.font = `500 ${H * 0.28}px ${mono}`; c.textBaseline = "middle";
          c.fillText(`✓ receipt ${j + 1} on NEAR`, W * 0.07, H / 2);
        });
        s.position.set(MODEL[0], -1.72 - j * 0.48, -0.05);
        return s;
      });

      const ring = new THREE.Mesh(new THREE.TorusGeometry(0.62, 0.035, 16, 96), new THREE.MeshStandardMaterial({ color: 0xcfe0ff, emissive: 0x4d92ff, emissiveIntensity: 0.6, metalness: 0.4, roughness: 0.3, transparent: true }));
      ring.position.set(...RING);
      ring.rotation.y = 0.5;

      const nft = panel(NFT.w, NFT.h, (c, W, H) => {
        const g = c.createRadialGradient(W / 2, -H * 0.25, W * 0.1, W / 2, -H * 0.25, H * 0.95);
        g.addColorStop(0, "#0a1247"); g.addColorStop(0.55, "#2450cf"); g.addColorStop(0.72, "#9fb8ee"); g.addColorStop(0.78, "#0d1440"); g.addColorStop(1, "#0b1030");
        frame(c, W, H, g, "rgba(220,230,255,0.5)", 0.07);
        c.fillStyle = "#ffffff"; c.font = `300 ${W * 0.1}px ${sans}`; c.textBaseline = "top";
        c.fillText("Agent NFT", W * 0.08, H * 0.06);
        c.fillStyle = "#dfe6ff"; c.font = `400 ${W * 0.038}px ${mono}`;
        ["Hyperliquid account", "margin and credits", "signed track record", "live price, 5% royalty"].forEach((t, k) => c.fillText(`→ ${t}`, W * 0.08, H * (0.2 + k * 0.055)));
      });
      nft.position.set(NFT.x, NFT.y, -0.3);

      // A live price line inside the account card.
      const LINE = 90;
      const lineGeo = new THREE.BufferGeometry();
      lineGeo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(LINE * 3), 3));
      const line = new THREE.Line(lineGeo, new THREE.LineBasicMaterial({ color: 0x7db6ff, transparent: true }));
      world.add(reserve, card, model, ...slips, ring, nft, line);

      const fades: { obj: THREE.Mesh | THREE.Line; lo: number; hi: number }[] = [
        { obj: reserve, lo: 0, hi: 0 },
        { obj: card, lo: 1, hi: 3 },
        { obj: model, lo: 2, hi: 2 },
        { obj: slips[0], lo: 2, hi: 3 },
        { obj: slips[1], lo: 2, hi: 3 },
        { obj: ring, lo: 3, hi: 3 },
        { obj: nft, lo: 4, hi: 4 },
        { obj: line, lo: 2, hi: 3 },
      ];

      const L = layout();
      const seeds = Array.from({ length: N }, () => Math.random());
      const dummy = new THREE.Object3D();
      const a3 = new THREE.Vector3(), b3 = new THREE.Vector3(), c3 = new THREE.Vector3(), ctrl = new THREE.Vector3(), v = new THREE.Vector3();

      const size = () => {
        const w = cv.clientWidth, h = cv.clientHeight;
        renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        camera.position.set(0, 0, camera.aspect < 1.05 ? 10.6 / camera.aspect : 10.6);
        camera.updateProjectionMatrix();
      };
      size();
      window.addEventListener("resize", size);

      const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      const tagEls = Array.from(tagBox.children) as HTMLElement[];
      let progress = 0, shown = -1;
      const start = performance.now();

      const tick = () => {
        const rect = el.getBoundingClientRect();
        const span = rect.height - window.innerHeight;
        const target = clamp01(-rect.top / span) * (CHAPTERS.length - 1);
        progress += (target - progress) * (still ? 1 : 0.16);
        if (rect.bottom > 0 && rect.top < window.innerHeight) {
          const time = still ? 0 : (performance.now() - start) / 1000;
          const seg = Math.min(Math.floor(progress), CHAPTERS.length - 2);
          const local = progress - seg;
          world.rotation.y = Math.sin(time * 0.35) * 0.1;
          world.rotation.x = Math.sin(time * 0.27) * 0.04;
          world.updateMatrixWorld();

          for (let i = 0; i < N; i++) {
            const A = L[seg][i], B = L[seg + 1][i];
            const t = smooth(clamp01((local - seeds[i] * 0.3) / 0.7));
            a3.set(...A.pos); b3.set(...B.pos);
            const flips = A.usdc !== B.usdc;
            if (flips && seg === 2) {
              // Unspent credits travel through the Intents ring on their way into the account.
              ctrl.set(RING[0], RING[1], RING[2]).multiplyScalar(2).sub(a3.clone().add(b3).multiplyScalar(0.5));
              c3.copy(a3).multiplyScalar((1 - t) ** 2).add(ctrl.clone().multiplyScalar(2 * (1 - t) * t)).add(b3.clone().multiplyScalar(t * t));
            } else {
              c3.copy(a3).lerp(b3, t);
              c3.z += Math.sin(t * Math.PI) * 0.8 * Math.min(1, a3.distanceTo(b3) / 2);
            }
            c3.y += Math.sin(time * 1.6 + seeds[i] * 12) * 0.03;
            const scale = A.scale + (B.scale - A.scale) * t;
            const toUsdc = flips ? (B.usdc ? clamp01((t - 0.5) * 2) : clamp01(1 - t * 2)) : A.usdc ? 1 : 0;
            const toCredit = flips ? (B.usdc ? clamp01(1 - t * 2) : clamp01((t - 0.5) * 2)) : A.usdc ? 0 : 1;
            dummy.position.copy(c3);
            dummy.rotation.set(Math.sin(time * 0.9 + seeds[i] * 7) * 0.12, Math.sin(time * 1.3 + seeds[i] * 9) * 0.32 + (flips ? t * Math.PI * 2 : 0), 0);
            dummy.scale.setScalar(Math.max(scale * toUsdc, 1e-4));
            dummy.updateMatrix();
            usdc.setMatrixAt(i, dummy.matrix);
            dummy.scale.setScalar(Math.max(scale * toCredit, 1e-4));
            dummy.updateMatrix();
            credit.setMatrixAt(i, dummy.matrix);
          }
          usdc.instanceMatrix.needsUpdate = true;
          credit.instanceMatrix.needsUpdate = true;

          const pos = lineGeo.attributes.position as THREE.BufferAttribute;
          for (let k = 0; k < LINE; k++) {
            const u = k / (LINE - 1), x = u * 5 + time * 0.9;
            const y = 0.45 * Math.sin(x * 1.3) + 0.22 * Math.sin(x * 3.1 + 1.2) + 0.09 * Math.sin(x * 7.7) + u * 0.35;
            pos.setXYZ(k, CARD.x - 1.62 + u * 3.24, 0.7 + y * 0.55, 0.02);
          }
          pos.needsUpdate = true;
          ring.rotation.z = time * 0.6;

          for (const f of fades) {
            const vis = THREE.MathUtils.smoothstep(progress, f.lo - 0.55, f.lo - 0.15) * (1 - THREE.MathUtils.smoothstep(progress, f.hi + 0.15, f.hi + 0.55));
            (f.obj.material as THREE.Material).opacity = vis;
            f.obj.visible = vis > 0.01;
          }
          renderer.render(scene, camera);

          const chapter = Math.round(progress);
          if (chapter !== shown) { shown = chapter; setActive(chapter); }
          const settled = Math.abs(progress - chapter) < 0.2;
          let n = 0;
          CHAPTERS.forEach((ch, ci) => ch.tags.forEach((tg) => {
            const tag = tagEls[n++];
            v.set(...tg.at).applyMatrix4(world.matrixWorld).project(camera);
            const x = Math.min(((v.x + 1) / 2) * cv.clientWidth, cv.clientWidth - tag.offsetWidth - 8);
            tag.style.transform = `translate(${x}px, ${((1 - v.y) / 2) * cv.clientHeight}px) translateY(-50%)`;
            tag.classList.toggle("on", ci === chapter && settled);
          }));
        }
        raf = requestAnimationFrame(tick);
      };
      tick();

      cleanup.push(() => {
        window.removeEventListener("resize", size);
        scene.traverse((o) => {
          const m = o as THREE.Mesh;
          m.geometry?.dispose();
          (Array.isArray(m.material) ? m.material : m.material ? [m.material] : []).forEach((mat) => { (mat as THREE.MeshBasicMaterial).map?.dispose(); mat.dispose(); });
        });
      });
    });

    return () => {
      disposed = true;
      cancelAnimationFrame(raf);
      cleanup.forEach((f) => f());
      renderer.dispose();
    };
  }, []);

  return (
    <section className="story" ref={section} id="how">
      <div className="story-pin">
        <div className="story-canvas">
          <canvas ref={canvas} aria-hidden />
          <div ref={tags} aria-hidden>
            {CHAPTERS.flatMap((c) => c.tags.map((t) => <span className="tag" key={c.label + t.text}>{t.text}</span>))}
          </div>
        </div>
        <div className="story-text">
          {CHAPTERS.map((c, i) => (
            <article className={`chapter ${i === active ? "on" : ""}`} key={c.label} aria-hidden={i !== active}>
              <span className="arrow">{c.label}</span>
              <h3>{c.title}</h3>
              <p>{c.body}</p>
            </article>
          ))}
        </div>
        <div className="ticks" aria-hidden>
          {CHAPTERS.map((c, i) => <i key={c.label} className={i <= active ? "on" : ""} />)}
        </div>
      </div>
    </section>
  );
}

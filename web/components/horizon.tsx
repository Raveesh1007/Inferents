"use client";
import { useEffect, useRef } from "react";
import * as THREE from "three";

// The hero's limb of light, drawn as one fragment shader. The CSS gradient under it is the fallback.
const FRAG = /* glsl */ `
precision highp float;
uniform vec2 uRes;
uniform float uTime;
varying vec2 vUv;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}

void main() {
  vec2 p = vec2(vUv.x, 1.0 - vUv.y);
  float aspect = uRes.x / uRes.y;
  // A wide ellipse centred above the viewport; its lower edge is the bright limb. Narrow screens get a wider one.
  float rx = mix(1.25, 0.72, clamp((aspect - 0.5) / 1.1, 0.0, 1.0));
  float breathe = (noise(vec2(p.x * 1.6, uTime * 0.06)) - 0.5) * 0.05;
  float d = length((p - vec2(0.5, -0.10)) / vec2(rx, 0.90 + breathe));

  vec3 navy = vec3(0.027, 0.039, 0.141);
  vec3 deep = vec3(0.040, 0.075, 0.300);
  vec3 blue = vec3(0.140, 0.300, 0.820);
  vec3 sky = vec3(0.400, 0.560, 0.930);
  vec3 pale = vec3(0.700, 0.770, 0.930);
  vec3 white = vec3(0.880, 0.900, 0.960);

  vec3 col = mix(navy, deep, smoothstep(0.22, 0.55, d));
  col = mix(col, blue, smoothstep(0.42, 0.74, d));
  col = mix(col, sky, smoothstep(0.68, 0.88, d));
  col = mix(col, pale, smoothstep(0.84, 0.96, d));
  col = mix(col, white, smoothstep(0.93, 1.00, d));
  // Outside the limb the light falls away through a grey violet into the night.
  vec3 dusk = mix(white, vec3(0.36, 0.38, 0.56), smoothstep(1.00, 1.07, d));
  col = mix(col, dusk, step(1.0, d));
  col = mix(col, navy, smoothstep(1.03, 1.19, d));

  col += (hash(gl_FragCoord.xy + fract(uTime) * 61.0) - 0.5) * 0.022;
  gl_FragColor = vec4(col, 1.0);
}`;

const VERT = /* glsl */ `varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position, 1.0); }`;

export function Horizon() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: "low-power" });
    } catch {
      canvas.style.display = "none";
      return;
    }
    const uniforms = { uRes: { value: new THREE.Vector2(1, 1) }, uTime: { value: 0 } };
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({ uniforms, vertexShader: VERT, fragmentShader: FRAG }));
    scene.add(mesh);
    const camera = new THREE.Camera();

    const size = () => {
      const { clientWidth: w, clientHeight: h } = canvas;
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5));
      renderer.setSize(w, h, false);
      uniforms.uRes.value.set(w, h);
    };
    size();
    window.addEventListener("resize", size);

    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let raf = 0;
    let visible = true;
    const io = new IntersectionObserver(([e]) => { visible = e.isIntersecting; });
    io.observe(canvas);
    const start = performance.now();
    const frame = () => {
      if (visible) {
        uniforms.uTime.value = (performance.now() - start) / 1000;
        renderer.render(scene, camera);
      }
      if (!still) raf = requestAnimationFrame(frame);
    };
    frame();

    return () => {
      cancelAnimationFrame(raf);
      io.disconnect();
      window.removeEventListener("resize", size);
      mesh.geometry.dispose();
      (mesh.material as THREE.Material).dispose();
      renderer.dispose();
    };
  }, []);

  return (
    <div className="horizon" aria-hidden>
      <canvas ref={ref} />
    </div>
  );
}

"use client";
import { useEffect, useRef } from "react";
import * as THREE from "three";

// The page's living background: slow currents of blue light with contour lines drawn through them, like a depth
// chart of the market the agents trade in. It sits behind every section; the hero's horizon covers it at the top.
const FRAG = /* glsl */ `
precision highp float;
uniform vec2 uRes;
uniform float uTime;
uniform float uScroll;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  for (int i = 0; i < 5; i++) { v += a * noise(p); p = p * 2.03 + vec2(1.7, 9.2); a *= 0.5; }
  return v;
}

void main() {
  vec2 uv = gl_FragCoord.xy / uRes.y;
  float t = uTime * 0.07;
  vec2 drift = vec2(0.0, uScroll * 0.35);
  vec2 q = vec2(fbm(uv * 1.1 + drift + vec2(0.0, t)), fbm(uv * 1.1 + drift + vec2(5.2, -t * 0.8)));
  float f = fbm(uv * 1.4 + drift + 2.4 * q + vec2(t * 0.5, 0.0));

  vec3 navy = vec3(0.027, 0.039, 0.141);
  vec3 deep = vec3(0.048, 0.085, 0.300);
  vec3 blue = vec3(0.150, 0.320, 0.860);
  vec3 col = mix(navy, deep, smoothstep(0.38, 0.78, f));
  col = mix(col, blue, smoothstep(0.66, 0.98, f) * 0.5);

  // Contour lines of the same field, drifting slowly downhill.
  float k = f * 11.0 - t * 1.6;
  float line = 1.0 - smoothstep(0.0, 1.4, abs(fract(k) - 0.5) / max(fwidth(k), 1e-4));
  col += vec3(0.45, 0.6, 1.0) * line * (0.035 + 0.09 * smoothstep(0.45, 0.85, f));

  col += (hash(gl_FragCoord.xy + fract(uTime) * 53.0) - 0.5) * 0.018;
  gl_FragColor = vec4(col, 1.0);
}`;

const VERT = /* glsl */ `void main() { gl_Position = vec4(position, 1.0); }`;

export function Drift() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: "low-power" });
    } catch {
      return;
    }
    const uniforms = { uRes: { value: new THREE.Vector2(1, 1) }, uTime: { value: 0 }, uScroll: { value: 0 } };
    const scene = new THREE.Scene();
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({ uniforms, vertexShader: VERT, fragmentShader: FRAG }));
    scene.add(mesh);
    const camera = new THREE.Camera();

    // A soft field reads the same at reduced resolution and costs far less.
    const size = () => {
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1) * 0.75);
      renderer.setSize(window.innerWidth, window.innerHeight, false);
      uniforms.uRes.value.set(window.innerWidth * renderer.getPixelRatio(), window.innerHeight * renderer.getPixelRatio());
    };
    size();
    window.addEventListener("resize", size);

    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let raf = 0;
    let scroll = 0;
    const start = performance.now();
    const frame = () => {
      scroll += (window.scrollY / window.innerHeight - scroll) * 0.08;
      uniforms.uScroll.value = scroll;
      uniforms.uTime.value = still ? 0 : (performance.now() - start) / 1000;
      renderer.render(scene, camera);
      if (!still) raf = requestAnimationFrame(frame);
    };
    frame();

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", size);
      mesh.geometry.dispose();
      (mesh.material as THREE.Material).dispose();
      renderer.dispose();
    };
  }, []);

  return <canvas ref={ref} className="drift" aria-hidden />;
}

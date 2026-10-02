(() => {
  const layer = document.querySelector(".fish-layer");
  const leftFish = document.querySelector(".swimmer-left");
  const rightFish = document.querySelector(".swimmer-right");
  const noise = document.querySelector("#water-noise");
  const displacement = document.querySelector("#water-displacement");

  if (!layer || !leftFish || !rightFish || !noise || !displacement) return;

  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  const fish = [
    {
      el: leftFish,
      direction: -1,
      phase: 0.18,
      baseDuration: 13.8,
      bobPhase: 0.4,
      speedPhase: 0.2,
      flip: 1
    },
    {
      el: rightFish,
      direction: 1,
      phase: 0.67,
      baseDuration: 15.2,
      bobPhase: 2.1,
      speedPhase: 2.7,
      flip: -1
    }
  ];

  let last = performance.now();

  function speedFactor(p, offset) {
    // Always positive, but clearly accelerates/decelerates through the pass.
    return (
      1 +
      0.34 * Math.sin(p * Math.PI * 2 + offset) +
      0.16 * Math.sin(p * Math.PI * 4 + offset * 0.63)
    );
  }

  function render(now) {
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;

    const layerRect = layer.getBoundingClientRect();
    let combinedSpeed = 0;

    for (const f of fish) {
      const rect = f.el.getBoundingClientRect();
      const w = rect.width || Math.min(window.innerWidth * 0.42, 620);
      const factor = speedFactor(f.phase, f.speedPhase);
      const normalizedVelocity = Math.max(0.5, factor);
      combinedSpeed += normalizedVelocity;

      f.phase = (f.phase + (dt / f.baseDuration) * factor) % 1;

      // Fully outside at both ends.
      const start = f.direction < 0 ? layerRect.width + w + 30 : -w - 30;
      const end = f.direction < 0 ? -w - 30 : layerRect.width + w + 30;
      const x = start + (end - start) * f.phase;

      // Swim motion also responds to velocity: faster = tighter/more energetic body motion.
      const energy = 0.78 + normalizedVelocity * 0.22;
      const bob = Math.sin(f.phase * Math.PI * 8 + f.bobPhase) * 10 * energy;
      const roll = Math.sin(f.phase * Math.PI * 8 + f.bobPhase + 0.8) * 1.8 * energy;
      const skew = Math.sin(f.phase * Math.PI * 12 + f.bobPhase) * 0.75 * energy;
      const squash = 1 + Math.sin(f.phase * Math.PI * 12 + f.bobPhase + 1.1) * 0.014 * energy;

      f.el.style.transform =
        `translate3d(${x}px, ${bob}px, 0) scaleX(${f.flip}) rotate(${roll}deg) skewY(${skew}deg) scaleY(${squash})`;
    }

    const avgSpeed = combinedSpeed / fish.length;
    const turbulenceX = 0.0055 + avgSpeed * 0.0022;
    const turbulenceY = 0.015 + avgSpeed * 0.007;
    const distortion = 7 + avgSpeed * 6.5;

    // Faster fish = tighter ripples and more displacement.
    noise.setAttribute("baseFrequency", `${turbulenceX.toFixed(4)} ${turbulenceY.toFixed(4)}`);
    displacement.setAttribute("scale", distortion.toFixed(2));

    // Small whole-water movement that also responds to speed.
    const t = now / 1000;
    const driftX = Math.sin(t * (0.55 + avgSpeed * 0.08)) * (0.25 + avgSpeed * 0.12);
    const driftY = Math.cos(t * (0.48 + avgSpeed * 0.06)) * (0.18 + avgSpeed * 0.1);
    const stretch = 1 + Math.sin(t * 0.7) * 0.0025 * avgSpeed;
    layer.style.transform = `translate3d(${driftX}%, ${driftY}%, 0) scale(${stretch}, ${2 - stretch})`;

    requestAnimationFrame(render);
  }

  function staticLayout() {
    const layerRect = layer.getBoundingClientRect();
    const leftW = leftFish.getBoundingClientRect().width;
    const rightW = rightFish.getBoundingClientRect().width;
    leftFish.style.transform = `translate3d(${layerRect.width * 0.62}px, 0, 0)`;
    rightFish.style.transform = `translate3d(${layerRect.width * 0.1 - rightW}px, 0, 0) scaleX(-1)`;
    noise.setAttribute("baseFrequency", "0.007 0.02");
    displacement.setAttribute("scale", "9");
    layer.style.transform = "none";
  }

  if (reducedMotion.matches) {
    staticLayout();
  } else {
    requestAnimationFrame(render);
  }

  reducedMotion.addEventListener?.("change", () => location.reload());
})();

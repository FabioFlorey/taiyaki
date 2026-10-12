(() => {
  const layer = document.querySelector(".fish-layer");
  const leftFish = document.querySelector(".swimmer-left");
  const rightFish = document.querySelector(".swimmer-right");
  const noise = document.querySelector("#water-noise");
  const displacement = document.querySelector("#water-displacement");

  if (!layer || !leftFish || !rightFish || !noise || !displacement) return;

  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Angles are measured from the direction of horizontal travel.
  // Positive angles swim down the page; negative angles swim up.
  // A fresh angle is picked for each crossing, without changing course mid-screen.
  const fish = [
    {
      el: leftFish,
      direction: 1,
      angleDeg: 45,
      lane: 0.34,
      phase: 0.18,
      baseDuration: 13.8,
      bobPhase: 0.4,
      speedPhase: 0.2,
      flip: -1
    },
    {
      el: rightFish,
      direction: -1,
      angleDeg: -23,
      lane: 0.70,
      phase: 0.67,
      baseDuration: 15.2,
      bobPhase: 2.1,
      speedPhase: 2.7,
      flip: 1
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

  function positionFish(f, bob = 0, roll = 0, skew = 0, squash = 1) {
    // Offset dimensions are unaffected by the transforms applied in prior frames.
    const width = f.el.offsetWidth;
    const height = f.el.offsetHeight;
    const sceneWidth = layer.clientWidth;
    const sceneHeight = layer.clientHeight;
    const angle = (f.angleDeg * Math.PI) / 180;
    const vx = f.direction * Math.cos(angle);
    const vy = Math.sin(angle);
    const originX = sceneWidth / 2;
    const originY = sceneHeight * f.lane;

    // Start and end outside the viewport, including the rotated sprite.
    const padding = Math.hypot(width, height) / 2 + 32;
    function edgeDistance(sign) {
      const dx = sign * vx;
      const dy = sign * vy;
      const x = dx > 0 ? (sceneWidth + padding - originX) / dx
        : dx < 0 ? (-padding - originX) / dx : Infinity;
      const y = dy > 0 ? (sceneHeight + padding - originY) / dy
        : dy < 0 ? (-padding - originY) / dy : Infinity;
      return Math.min(x, y);
    }

    const before = edgeDistance(-1);
    const after = edgeDistance(1);
    const distance = -before + (before + after) * f.phase;
    const x = originX + vx * distance - width / 2 - vy * bob;
    const y = originY + vy * distance - height / 2 + vx * bob;

    // The image faces left before transforms: mirror when swimming right.
    const heading = (f.direction === 1 ? f.angleDeg : -f.angleDeg) + roll;
    f.el.style.transform =
      `translate3d(${x}px, ${y}px, 0) rotate(${heading}deg) scaleX(${f.flip}) skewY(${skew}deg) scaleY(${squash})`;
  }

  function render(now) {
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;

    let combinedSpeed = 0;

    for (const f of fish) {
      const factor = speedFactor(f.phase, f.speedPhase);
      const normalizedVelocity = Math.max(0.5, factor);
      combinedSpeed += normalizedVelocity;

      const nextPhase = f.phase + (dt / f.baseDuration) * factor;
      f.phase = nextPhase % 1;
      if (nextPhase >= 1) {
        f.angleDeg = Math.random() * 110 - 55;
      }

      // Bob perpendicular to the path, not only along the page's Y axis.
      const energy = 0.78 + normalizedVelocity * 0.22;
      const bob = Math.sin(f.phase * Math.PI * 8 + f.bobPhase) * 10 * energy;
      const roll = Math.sin(f.phase * Math.PI * 8 + f.bobPhase + 0.8) * 1.8 * energy;
      const skew = Math.sin(f.phase * Math.PI * 12 + f.bobPhase) * 0.75 * energy;
      const squash = 1 + Math.sin(f.phase * Math.PI * 12 + f.bobPhase + 1.1) * 0.014 * energy;
      positionFish(f, bob, roll, skew, squash);
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
    for (const f of fish) {
      positionFish({ ...f, phase: 0.5 });
    }
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

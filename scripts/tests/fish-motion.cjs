#!/usr/bin/env node
// Deterministic regression for the animated, diagonally moving site fish.
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const root = path.resolve(__dirname, "../..");
const source = fs.readFileSync(path.join(root, "site/assets/js/main.js"), "utf8");

function simulation(reducedMotion, width = 1400, height = 800) {
  const frames = [];
  const node = () => ({
    style: {},
    offsetWidth: 500,
    offsetHeight: 349,
    setAttribute() {},
  });
  const layer = { ...node(), clientWidth: width, clientHeight: height };
  const upper = node();
  const lower = node();
  const elements = new Map([
    [".fish-layer", layer],
    [".swimmer-left", upper],
    [".swimmer-right", lower],
    ["#water-noise", node()],
    ["#water-displacement", node()],
  ]);
  const sandbox = {
    document: { querySelector: name => elements.get(name) },
    window: { matchMedia: () => ({ matches: reducedMotion, addEventListener() {} }) },
    performance: { now: () => 0 },
    requestAnimationFrame: fn => frames.push(fn),
    Math: Object.assign(Object.create(Math), { random: () => 65 / 110 }),
    location: { reload: () => { throw Error("Unexpected page reload"); } },
  };
  vm.runInNewContext(source, sandbox, { filename: "main.js" });

  function tick(now) {
    assert.equal(frames.length, 1, "expected one animation callback");
    frames.shift()(now);
  }

  function pose(el) {
    const transform = el.style.transform;
    assert.ok(transform, "fish must have a transform");
    const position = /translate3d\(([-+\d.eE]+)px, ([-+\d.eE]+)px, 0\)/.exec(transform);
    const orientation = /rotate\(([-+\d.eE]+)deg\) scaleX\(([-+\d.eE]+)\)/.exec(transform);
    assert.ok(position && orientation, "unexpected transform: " + transform);
    const result = {
      x: Number(position[1]), y: Number(position[2]),
      rotation: Number(orientation[1]), flip: Number(orientation[2]),
    };
    assert.ok(Object.values(result).every(Number.isFinite), "nonfinite fish pose");
    return result;
  }
  return { tick, pose, upper, lower, frames };
}

function near(actual, expected, tolerance, message) {
  assert.ok(Math.abs(actual - expected) < tolerance,
    `${message}: expected ${expected} ± ${tolerance}, got ${actual}`);
}

const normal = simulation(false);
normal.tick(0);
const upperBefore = normal.pose(normal.upper);
const lowerBefore = normal.pose(normal.lower);
for (let i = 1; i <= 40; i++) normal.tick(i * 50);
const upperAfter = normal.pose(normal.upper);
const lowerAfter = normal.pose(normal.lower);
assert.ok(upperAfter.x > upperBefore.x && upperAfter.y > upperBefore.y,
  "45° fish must travel down and right");
assert.ok(lowerAfter.x < lowerBefore.x && lowerAfter.y < lowerBefore.y,
  "−23° fish must travel up and left");
near((upperAfter.y - upperBefore.y) / (upperAfter.x - upperBefore.x), 1, 0.3,
  "45° trajectory slope");
near((lowerAfter.y - lowerBefore.y) / (lowerAfter.x - lowerBefore.x),
  Math.tan(23 * Math.PI / 180), 0.3, "−23° trajectory slope");
assert.equal(upperAfter.flip, -1, "right-moving sprite must face right");
assert.equal(lowerAfter.flip, 1, "left-moving sprite must face left");
near(upperAfter.rotation, 45, 2, "45° sprite facing");
near(lowerAfter.rotation, 23, 2, "−23° leftward sprite facing");

// A complete crossing must select another arbitrary angle (10° with seeded RNG).
for (let i = 41; i <= 400; i++) normal.tick(i * 50);
near(normal.pose(normal.upper).rotation, 10, 2, "next rightward crossing");
near(normal.pose(normal.lower).rotation, -10, 2, "next leftward crossing");

const staticScene = simulation(true);
assert.equal(staticScene.frames.length, 0, "reduced motion must not animate");
near(staticScene.pose(staticScene.upper).rotation, 45, 0.001, "static 45° pose");
near(staticScene.pose(staticScene.lower).rotation, 23, 0.001, "static −23° pose");

const narrow = simulation(false, 400, 350);
narrow.tick(0);
narrow.tick(50);
narrow.pose(narrow.upper);
narrow.pose(narrow.lower);

console.log("Fish motion PASS: diagonal 45° / −23°, angle 10° after loop, sprite facing, reduced motion and narrow viewport");

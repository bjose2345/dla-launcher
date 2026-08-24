import { readFileSync } from "node:fs";

const criteria = process.argv.slice(2);
if (criteria.length === 0 || criteria.length % 2 !== 0) {
  console.error("usage: android-ui-node.mjs <attribute> <exact-value> [...]");
  process.exit(2);
}

const document = readFileSync(0, "utf8");
const nodes = document.match(/<node\b[^>]*>/g) ?? [];
const node = nodes.find((candidate) =>
  criteria.every((value, index) =>
    index % 2 !== 0 || candidate.includes(`${value}="${criteria[index + 1]}"`),
  ) &&
  !candidate.includes('enabled="false"'),
);
if (!node) process.exit(1);

const bounds = node.match(/bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
if (!bounds) process.exit(1);
const [, left, top, right, bottom] = bounds.map(Number);
console.log(`${Math.floor((left + right) / 2)} ${Math.floor((top + bottom) / 2)}`);

import { execFileSync } from "node:child_process";
import { fixtures, root } from "./support.mjs";

export default function setup() {
  execFileSync("cargo", ["run", "--locked", "--example", "dashboard-preview", "--", fixtures], {
    cwd: root,
    env: { ...process.env, TZ: "UTC0" },
    stdio: "inherit",
  });
}

#!/usr/bin/env bun
/**
 * Verify that the workspaces pin every GPUI crate to an exact version.
 *
 * The published mhr-gpui-kit crates carry the workspace's requirements to
 * crates.io. A caret requirement lets a new release of one `mhr-gpui*` crate
 * reach an application beside the others and mhr-gpui-kit-component, none of
 * which were built with it. Every GPUI crate has to stay on one `=x.y.z`
 * requirement, so a kit release resolves exactly the crates it was tested
 * with. The crates are released one by one, so their versions may differ.
 * The kit's own `mhr-gpui-kit*` crates release together and are pinned the
 * same way. The `gpui-pre-reqwest` fork of reqwest that GPUI's HTTP client
 * builds on only needs to be exact too. The scripting shell has a workspace
 * of its own in `shell/` and repeats the GPUI pins, so both manifests are
 * checked. A path dependency without a version, like the shell's kit crates,
 * is not resolved from a registry and has nothing to pin.
 *
 *     bun script/check-gpui-pin.ts [path/to/Cargo.toml]
 */
import { readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = resolve(import.meta.dir, "..");
const MANIFESTS = ["Cargo.toml", "shell/Cargo.toml"];
const GPUI_PREFIX = "mhr-";
const EXTRA_PINNED = new Set(["gpui-pre-reqwest"]);
const EXACT_VERSION = /^=(\d+\.\d+\.\d+)$/;

interface Dependency {
  alias: string;
  package: string;
  version: string | undefined;
}

interface Manifest {
  workspace?: { dependencies?: Record<string, unknown> };
}

function isPinned(pkg: string): boolean {
  return pkg.startsWith(GPUI_PREFIX) || EXTRA_PINNED.has(pkg);
}

function versionOf(spec: unknown): string | undefined {
  if (typeof spec === "string") return spec;
  if (typeof spec !== "object" || spec === null) return undefined;
  const { version } = spec as { version?: unknown };
  return typeof version === "string" ? version : undefined;
}

function readManifest(manifestPath: string): Manifest {
  return Bun.TOML.parse(readFileSync(manifestPath, "utf8")) as Manifest;
}

function gpuiDependencies(manifest: Manifest): Dependency[] {
  const dependencies: Dependency[] = [];
  for (const [alias, spec] of Object.entries(manifest.workspace?.dependencies ?? {})) {
    if (typeof spec !== "object" || spec === null) continue;
    const { package: pkg } = spec as { package?: unknown };
    if (typeof pkg !== "string" || !isPinned(pkg)) continue;
    const version = versionOf(spec);
    if (version === undefined && typeof (spec as { path?: unknown }).path === "string") continue;
    dependencies.push({ alias, package: pkg, version });
  }
  return dependencies;
}

function describe(dependency: Dependency): string {
  return `${dependency.alias} = { package = "${dependency.package}", version = ${JSON.stringify(dependency.version ?? "")} }`;
}

function problems(dependencies: Dependency[]): string[] {
  const found: string[] = [];
  if (dependencies.length === 0) found.push(`no \`${GPUI_PREFIX}*\` dependency in [workspace.dependencies]`);
  for (const dependency of dependencies) {
    if (dependency.version === undefined || !EXACT_VERSION.test(dependency.version)) {
      found.push(`${describe(dependency)} is not an exact \`=x.y.z\` requirement`);
    }
  }
  return found;
}

const manifestPaths = process.argv[2] ? [process.argv[2]] : MANIFESTS.map((name) => join(REPO_ROOT, name));
let failed = false;
for (const manifestPath of manifestPaths) {
  const name = relative(REPO_ROOT, resolve(manifestPath));
  const found = problems(gpuiDependencies(readManifest(manifestPath)));
  if (found.length > 0) {
    failed = true;
    for (const problem of found) console.error(`::error file=${name}::${problem}`);
    console.error(`\n${name} must pin every GPUI crate to the exact release gpui-kit was built against.`);
  } else {
    console.log(`${name} pins the GPUI crates exactly`);
  }
}
if (failed) process.exit(1);

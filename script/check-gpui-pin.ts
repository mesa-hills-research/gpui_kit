#!/usr/bin/env bun
/**
 * Verify that the workspace pins every GPUI crate to an exact version.
 *
 * The published mhr-gpui-kit crates carry the workspace's requirements to
 * crates.io. A caret requirement lets a new release of one `mhr-gpui*` crate
 * reach an application beside the others and mhr-gpui-kit-component, none of
 * which were built with it. Every GPUI crate has to stay on one `=x.y.z`
 * requirement, so a kit release resolves exactly the crates it was tested
 * with. The crates are released one by one, so their versions may differ.
 * The kit's own `mhr-gpui-kit*` crates release together and are pinned the
 * same way. The `gpui-pre-reqwest` fork of reqwest that GPUI's HTTP client
 * builds on only needs to be exact too.
 *
 *     bun script/check-gpui-pin.ts [path/to/Cargo.toml]
 */
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const REPO_ROOT = resolve(import.meta.dir, "..");
const MANIFEST = "Cargo.toml";
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
    dependencies.push({ alias, package: pkg, version: versionOf(spec) });
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

const manifestPath = process.argv[2] ?? join(REPO_ROOT, MANIFEST);
const manifest = readManifest(manifestPath);
const found = problems(gpuiDependencies(manifest));
if (found.length > 0) {
  for (const problem of found) console.error(`::error file=${MANIFEST}::${problem}`);
  console.error(`\n${MANIFEST} must pin every GPUI crate to the exact release gpui-kit was built against.`);
  process.exit(1);
}
console.log(`${MANIFEST} pins the GPUI crates exactly`);

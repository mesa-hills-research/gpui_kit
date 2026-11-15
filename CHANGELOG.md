# Changelog

Each release publishes six crates together at one version, and they pin each other exactly:
`mhr-gpui-kit`, `mhr-gpui-kit-component`, `mhr-gpui-kit-base`, `mhr-gpui-kit-writing`,
`mhr-gpui-kit-assets` and `mhr-gpui-kit-component-macros`.

## 0.8.1

The kit's crates compile together with any mix of their test features.

- `mhr-gpui-kit-component` failed to compile when another crate in the build turned on
  `test-support` in `mhr-gpui-kit-base` but not in the component crate. An app's tests did that
  when they depended on `mhr-gpui-kit-base` or `mhr-gpui-kit-writing` with `test-support`. The
  component crate now compiles with the feature on in either crate, in both or in neither.
- The `gpui-kit` and `gpui-kit-design-guides` skills send agents to docs.rs and to this
  repository's source, gallery and examples. They used to fetch pages from upstream GPUI Kit's
  site, which describe upstream's releases.
- The repository no longer carries upstream's web gallery, its WebAssembly example or upstream's
  agent files.

The six crates move to 0.8.1 together and still pin mhr-gpui 0.2.0.

## Earlier releases

- [0.8.0](https://github.com/mesa-hills-research/gpui_kit/releases/tag/v0.8.0)
- [0.7.17](https://github.com/mesa-hills-research/gpui_kit/releases/tag/v0.7.17)
- [0.7.16](https://github.com/mesa-hills-research/gpui_kit/releases/tag/v0.7.16)
- [0.7.15](https://crates.io/crates/mhr-gpui-kit/0.7.15)
- [0.7.14](https://crates.io/crates/mhr-gpui-kit/0.7.14)
- [0.7.13](https://crates.io/crates/mhr-gpui-kit/0.7.13)
- [0.7.12](https://crates.io/crates/mhr-gpui-kit/0.7.12)
- [0.7.11](https://crates.io/crates/mhr-gpui-kit/0.7.11)
- [0.7.10](https://crates.io/crates/mhr-gpui-kit/0.7.10)
- [0.7.9](https://crates.io/crates/mhr-gpui-kit/0.7.9)
- [0.7.8](https://crates.io/crates/mhr-gpui-kit/0.7.8)
- [0.7.7](https://crates.io/crates/mhr-gpui-kit/0.7.7)
- [0.7.6](https://crates.io/crates/mhr-gpui-kit/0.7.6)
- [0.7.5](https://crates.io/crates/mhr-gpui-kit/0.7.5)
- [0.7.4](https://crates.io/crates/mhr-gpui-kit/0.7.4)
- [0.7.3](https://crates.io/crates/mhr-gpui-kit/0.7.3)
- [0.7.2](https://crates.io/crates/mhr-gpui-kit/0.7.2)

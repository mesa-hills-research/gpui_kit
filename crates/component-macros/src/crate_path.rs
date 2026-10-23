use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// The package a standalone `mhr-gpui-kit-component` consumer takes GPUI from.
const GPUI_PACKAGE: &str = "mhr-gpui";
const KIT_PACKAGE: &str = "mhr-gpui-kit";
const COMPONENT_PACKAGE: &str = "mhr-gpui-kit-component";

type Lookup<'a> = &'a dyn Fn(&str) -> Result<FoundCrate, proc_macro_crate::Error>;

/// Looks up a package by its name in the manifest of the crate where a macro is expanded.
///
/// A dependency that keeps its package's name has no rename to read the crate name from, and
/// the crate is reached by its library name instead (`gpui_kit` for `mhr-gpui-kit`), which
/// the lookup would otherwise get wrong.
fn find(
    lookup: Lookup,
    package: &str,
    library: &str,
) -> Result<FoundCrate, proc_macro_crate::Error> {
    lookup(package).map(|found| match found {
        FoundCrate::Name(name) if name == package.replace('-', "_") => {
            FoundCrate::Name(library.into())
        }
        found => found,
    })
}

/// Resolve the GPUI API exposed to the crate where a macro is expanded.
///
/// `mhr-gpui-kit` is preferred because it re-exports GPUI and is the only direct
/// dependency required by kit consumers. The `mhr-gpui` package fallback
/// preserves standalone `mhr-gpui-kit-component` consumers, including dependencies
/// that rename that package to `gpui` (the conventional name).
pub(crate) fn gpui() -> syn::Result<TokenStream> {
    gpui_with(&crate_name)
}

fn gpui_with(lookup: Lookup) -> syn::Result<TokenStream> {
    match find(lookup, KIT_PACKAGE, "gpui_kit") {
        Ok(found) => Ok(found_crate_path(found)),
        Err(kit_error) => find(lookup, GPUI_PACKAGE, "gpui")
            .map(found_crate_path)
            .map_err(|gpui_error| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "IntoPlot requires a direct dependency on `{KIT_PACKAGE}` or `{GPUI_PACKAGE}`. \
                         Looking up {KIT_PACKAGE} failed ({kit_error}), and looking up {GPUI_PACKAGE} \
                         failed ({gpui_error})."
                    ),
                )
            }),
    }
}

fn found_crate_path(found: FoundCrate) -> TokenStream {
    match found {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
    }
}

/// Resolve the `mhr-gpui-kit-component` API exposed to the crate where a macro is
/// expanded, mirroring [`gpui`]: `mhr-gpui-kit` consumers reach it as
/// `gpui_kit::component`, standalone consumers as `gpui_component`, and the
/// crate itself as `crate`.
pub(crate) fn component() -> syn::Result<TokenStream> {
    component_with(&crate_name)
}

fn component_with(lookup: Lookup) -> syn::Result<TokenStream> {
    match find(lookup, KIT_PACKAGE, "gpui_kit") {
        Ok(found) => {
            let kit = found_crate_path(found);
            Ok(quote!(#kit::component))
        }
        Err(kit_error) => find(lookup, COMPONENT_PACKAGE, "gpui_component")
            .map(found_crate_path)
            .map_err(|component_error| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "IntoPlot requires a direct dependency on `{KIT_PACKAGE}` or \
                         `{COMPONENT_PACKAGE}`. Looking up {KIT_PACKAGE} failed ({kit_error}), \
                         and looking up {COMPONENT_PACKAGE} failed ({component_error})."
                    ),
                )
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro_crate::Error;

    /// A lookup that finds only the packages listed, under the given names.
    fn finds<'a>(
        found: &'a [(&'a str, FoundCrate)],
    ) -> impl Fn(&str) -> Result<FoundCrate, Error> + 'a {
        move |package| {
            found
                .iter()
                .find(|(name, _)| *name == package)
                .map(|(_, crate_name)| crate_name.clone())
                .ok_or(Error::CargoManifestDirNotSet)
        }
    }

    #[test]
    fn a_kit_dependency_is_preferred() {
        let found = [
            ("mhr-gpui-kit", FoundCrate::Name("gpui_kit".into())),
            ("mhr-gpui", FoundCrate::Name("gpui".into())),
        ];
        let path = gpui_with(&finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui_kit");
        let path = component_with(&finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui_kit :: component");
    }

    #[test]
    fn a_dependency_under_its_package_name_is_reached_by_the_library_name() {
        let found = [
            ("mhr-gpui-kit", FoundCrate::Name("mhr_gpui_kit".into())),
            (
                "mhr-gpui-kit-component",
                FoundCrate::Name("mhr_gpui_kit_component".into()),
            ),
        ];
        let path = gpui_with(&finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui_kit");
        let found = &found[1..];
        let path = component_with(&finds(found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui_component");
    }

    #[test]
    fn a_standalone_consumer_reaches_gpui_by_its_package() {
        let found = [("mhr-gpui", FoundCrate::Name("gpui".into()))];
        let path = gpui_with(&finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui");
    }

    #[test]
    fn the_old_gpui_snapshot_package_no_longer_counts() {
        let found = [("gpui-pre", FoundCrate::Name("gpui".into()))];
        let error = gpui_with(&finds(&found)).unwrap_err().to_string();
        assert!(error.contains("`mhr-gpui-kit` or `mhr-gpui`"), "{error}");
    }
}

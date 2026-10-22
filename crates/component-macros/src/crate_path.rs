use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// The package a standalone `gpui-component` consumer takes GPUI from.
const GPUI_PACKAGE: &str = "mhr-gpui";

/// Resolve the GPUI API exposed to the crate where a macro is expanded.
///
/// `gpui-kit` is preferred because it re-exports GPUI and is the only direct
/// dependency required by kit consumers. The `mhr-gpui` package fallback
/// preserves standalone `gpui-component` consumers, including dependencies
/// that rename that package to `gpui` (the conventional name).
pub(crate) fn gpui() -> syn::Result<TokenStream> {
    gpui_with(crate_name)
}

fn gpui_with(
    lookup: impl Fn(&str) -> Result<FoundCrate, proc_macro_crate::Error>,
) -> syn::Result<TokenStream> {
    match lookup("gpui-kit") {
        Ok(found) => Ok(found_crate_path(found)),
        Err(kit_error) => lookup(GPUI_PACKAGE)
            .map(found_crate_path)
            .map_err(|gpui_error| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "IntoPlot requires a direct dependency on `gpui-kit` or `{GPUI_PACKAGE}`. \
                         Looking up gpui-kit failed ({kit_error}), and looking up {GPUI_PACKAGE} \
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

/// Resolve the `gpui-component` API exposed to the crate where a macro is
/// expanded, mirroring [`gpui`]: `gpui-kit` consumers reach it as
/// `gpui_kit::component`, standalone consumers as `gpui_component`, and the
/// crate itself as `crate`.
pub(crate) fn component() -> syn::Result<TokenStream> {
    match crate_name("gpui-kit") {
        Ok(found) => {
            let kit = found_crate_path(found);
            Ok(quote!(#kit::component))
        }
        Err(kit_error) => crate_name("gpui-component").map(found_crate_path).map_err(
            |component_error| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "IntoPlot requires a direct dependency on `gpui-kit` or `gpui-component`: \
                         gpui-kit lookup failed: {kit_error}; gpui-component lookup failed: \
                         {component_error}"
                    ),
                )
            },
        ),
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
            ("gpui-kit", FoundCrate::Name("gpui_kit".into())),
            ("mhr-gpui", FoundCrate::Name("gpui".into())),
        ];
        let path = gpui_with(finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui_kit");
    }

    #[test]
    fn a_standalone_consumer_reaches_gpui_by_its_package() {
        let found = [("mhr-gpui", FoundCrate::Name("gpui".into()))];
        let path = gpui_with(finds(&found)).unwrap();
        assert_eq!(path.to_string(), ":: gpui");
    }

    #[test]
    fn the_old_gpui_snapshot_package_no_longer_counts() {
        let found = [("gpui-pre", FoundCrate::Name("gpui".into()))];
        let error = gpui_with(finds(&found)).unwrap_err().to_string();
        assert!(error.contains("`gpui-kit` or `mhr-gpui`"), "{error}");
    }
}

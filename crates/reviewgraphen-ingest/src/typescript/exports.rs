//! Same-file export-list recognition; recursive export resolution is a later cohort.

pub(crate) fn same_file_exported(source: &str, name: &str) -> bool {
    source.contains(&format!("export {{{name}"))
        || source.contains(&format!(" {name}}}"))
        || source.contains(&format!("export default {name}"))
}

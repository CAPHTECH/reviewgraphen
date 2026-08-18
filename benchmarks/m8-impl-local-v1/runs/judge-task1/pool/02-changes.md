# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `1b3e1e8b79ad9952`

Unified diff against the current `crates/reviewgraphen-cli/src/lib.rs`, with 25 lines of context on each side:

```diff
@@ -37,50 +37,62 @@
     fn failure(exit_code: u8, stderr: impl Into<String>) -> Self {
         Self {
             exit_code,
             stdout: Vec::new(),
             stderr: stderr.into(),
         }
     }
 }
 
 /// Executes the closed command surface. Unknown review forms, including every
 /// fixed-fixture form, are rejected before filesystem or report work starts.
 pub fn run(arguments: Vec<String>) -> CommandOutcome {
     match arguments.as_slice() {
         [
             command,
             request_flag,
             request_path,
             artifacts_flag,
             artifact_root,
         ] if command == "review"
             && request_flag == "--request"
             && artifacts_flag == "--artifacts" =>
         {
             generic_review(Path::new(request_path), Path::new(artifact_root))
         }
+        [
+            command,
+            artifacts_flag,
+            artifact_root,
+            request_flag,
+            request_path,
+        ] if command == "review"
+            && artifacts_flag == "--artifacts"
+            && request_flag == "--request" =>
+        {
+            generic_review(Path::new(request_path), Path::new(artifact_root))
+        }
         [command, subcommand] if command == "schema" && subcommand == "list" => schema_list(),
         [command, subcommand, name] if command == "schema" && subcommand == "print" => {
             schema_print(name)
         }
         [command, subcommand, path] if command == "schema" && subcommand == "validate" => {
             schema_validate(Path::new(path))
         }
         _ => CommandOutcome::failure(2, usage()),
     }
 }
 
 fn usage() -> &'static str {
     "usage: reviewgraphen review --request <request.json> --artifacts <fresh-absolute-dir> | schema list|print <schema-id>|validate <json-file>"
 }
 
 fn generic_review(request_path: &Path, artifact_root: &Path) -> CommandOutcome {
     let bytes = match bounded_regular_file(request_path) {
         Ok(bytes) => bytes,
         Err(error) => return CommandOutcome::failure(3, error),
     };
     let request: GenericReviewRequest = match serde_json::from_slice(&bytes) {
         Ok(request) => request,
         Err(_) => return CommandOutcome::failure(3, "invalid generic review request JSON"),
     };
     match run_generic_review(&request, artifact_root).and_then(|run| run.canonical_bytes()) {
```

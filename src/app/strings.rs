//! User-visible UI text, kept in one place to prepare for translation.

pub const APP_NAME: &str = "SimpleEditor";
pub const NO_PROJECT: &str = "No folder open";

pub const OPEN_FOLDER: &str = "Open folder…";
pub const OPEN_FILE: &str = "Open file…";
pub const NEW_FILE: &str = "New file";
pub const NEW_REQUEST: &str = "New HTTP request";
pub const SAVE: &str = "Save";
pub const SAVE_AS: &str = "Save as…";
pub const DONT_SAVE: &str = "Don't save";
pub const CANCEL: &str = "Cancel";
pub const DISMISS: &str = "Dismiss";
pub const OVERWRITE: &str = "Overwrite";
pub const THEME_SYSTEM: &str = "Theme: System";
pub const THEME_LIGHT: &str = "Theme: Light";
pub const THEME_DARK: &str = "Theme: Dark";

pub const SECTION_FILES: &str = "FILES";
pub const SECTION_HTTP: &str = "HTTP";
pub const LOADING: &str = "Loading…";
pub const EMPTY_FOLDER: &str = "Empty folder";
pub const NO_REQUESTS: &str = "No requests yet";

pub const WELCOME_TITLE: &str = "Escribe código. Prueba APIs. Así de simple.";
pub const WELCOME_HINT: &str = "Open a folder or a file, or start a new HTTP request.";

pub const UNSAVED_TITLE: &str = "Unsaved changes";
pub const UNSAVED_ONE: &str = "Do you want to save the changes to";
pub const UNSAVED_MANY: &str = "Some files have unsaved changes:";
pub const UNSAVED_HINT: &str = "Your changes will be lost if you don't save them.";
pub const SAVE_ALL: &str = "Save all";
pub const DISCARD_ALL: &str = "Discard all and quit";
pub const CONFLICT_TITLE: &str = "File changed on disk";
pub const CONFLICT_BODY: &str = "Another program changed this file after it was opened. Overwrite it with your version, save a copy elsewhere, or cancel and keep editing.";
pub const MODIFIED: &str = "modified";

pub const SEND: &str = "Send";
pub const CANCEL_REQUEST: &str = "Cancel";
pub const URL_HINT: &str = "https://api.example.com/items";
pub const BODY: &str = "Body";
pub const BODY_HINT: &str = "Request body (optional). Sent with the selected Content-Type.";
pub const RESPONSE: &str = "Response";
pub const RESPONSE_BODY: &str = "Body";
pub const RESPONSE_HEADERS: &str = "Headers";
pub const NOT_SENT: &str = "Send the request to see the response here.";
pub const SENDING: &str = "Sending… you can keep editing; Cancel stops the request.";
pub const CANCELLED: &str = "Request cancelled.";
pub const EMPTY_BODY: &str = "Empty body.";
pub const BINARY_BODY: &str = "Binary or non-UTF-8 body; not shown as text.";
pub const COPY_BODY: &str = "Copy body";
pub const SEND_SHORTCUT_HINT: &str = "Send (Cmd/Ctrl+Enter)";
pub const LONG_LINES_CLIPPED: &str =
    "Very long lines are clipped in this view; use Copy body for the full text.";

pub const SAVING: &str = "Saving…";
pub const HIGHLIGHT_OFF_LARGE: &str = "Highlighting off (large file)";
pub const DISK_CHANGED: &str = "This file was changed by another program. Reload it (your unsaved changes will be lost) or keep your version.";
pub const DISK_DELETED: &str = "This file was deleted or moved by another program. Your text is still here; save it to recreate the file.";
pub const RELOAD: &str = "Reload";
pub const KEEP_MINE: &str = "Keep my version";

pub const FIND_HINT: &str = "Find";
pub const REPLACE_HINT: &str = "Replace";
pub const MATCH_CASE: &str = "Match case";
pub const SEARCHING: &str = "Searching…";
pub const NO_RESULTS: &str = "No results";
pub const PREV_MATCH: &str = "Previous match (Shift+Enter, Shift+Cmd/Ctrl+G)";
pub const NEXT_MATCH: &str = "Next match (Enter, Cmd/Ctrl+G)";
pub const REPLACE: &str = "Replace";
pub const REPLACE_ALL: &str = "Replace all";
pub const CLOSE_FIND: &str = "Close (Esc)";
pub const TOO_MANY_MATCHES: &str = "Only the first 100000 matches are found and replaced.";

pub const RECOVERY_FOUND: &str =
    "Unsaved changes from a previous session that did not close normally were found:";
pub const RESTORE: &str = "Restore";
pub const DISCARD: &str = "Discard";

// ---- HTTP (H2) ----
pub const ENVIRONMENTS: &str = "Environments";
pub const ENVIRONMENT: &str = "Active environment for {{variables}}";
pub const NO_ENVIRONMENT: &str = "No environment";
pub const EDIT_ENVIRONMENTS: &str = "Edit environments and variables";
pub const ENV_HELP: &str = "Variables are used as {{name}} in URL, params, headers, body and auth. Precedence: request variables > active environment > project variables. Saved in .simpleeditor/ (readable JSON for Git); secret values are never written there.";
pub const PROJECT_VARIABLES: &str = "Project variables";
pub const NEW_ENVIRONMENT_HINT: &str = "New environment";
pub const ADD_ENVIRONMENT: &str = "Add environment";
pub const DELETE_ENVIRONMENT: &str = "Delete environment";
pub const NAME: &str = "Name";
pub const VALUE: &str = "Value";
pub const SECRET: &str = "Secret";
pub const REMEMBER: &str = "Remember";
pub const REMEMBER_HINT: &str = "Keep this secret between sessions in the";
pub const REMOVE_ROW: &str = "Remove";
pub const ADD_VARIABLE: &str = "+ Add variable";
pub const SECRET_NOTE: &str = "Secret values stay in memory for this session unless \"Remember\" is checked; they are never written to project files, history or exports.";
pub const NOT_SAVED: &str = "not saved";
pub const SAVE_REQUEST_HINT: &str = "Save to a collection (Cmd/Ctrl+S)";
pub const COPY_CURL: &str = "Copy as cURL";
pub const COPY_CURL_HINT: &str =
    "POSIX shell (bash, zsh) quoting; literal credentials are replaced with <omitted>";
pub const SEND_TO_FILE: &str = "Send and save response to file…";
pub const PARAMS: &str = "Params";
pub const HEADERS: &str = "Headers";
pub const AUTH: &str = "Auth";
pub const VARIABLES: &str = "Variables";
pub const SETTINGS: &str = "Settings";
pub const ADD_PARAM: &str = "+ Add param";
pub const ADD_HEADER: &str = "+ Add header";
pub const FORMAT_JSON: &str = "Format JSON";
pub const REQUEST_VARS_HELP: &str = "Request variables override the environment and project ones.";
pub const TIMEOUT_SECS: &str = "Timeout (seconds)";
pub const DOWNLOAD_LIMIT_MIB: &str = "Download limit for \"save to file\" (MiB)";
pub const SETTINGS_HELP: &str = "Redirects are followed up to 10 times; credentials are not sent to another origin. The response preview keeps up to 5 MiB (decompressed).";
pub const DOWNLOADING_TO: &str = "Saving the response to";
pub const ENABLED_HINT: &str = "Include in the request";
pub const TOKEN: &str = "Token";
pub const USERNAME: &str = "Username";
pub const PASSWORD: &str = "Password";
pub const AUTH_HELP: &str = "Tip: use a secret variable such as {{token}}; literal credentials are not saved in collections.";
pub const SAVED_TO: &str = "Saved to";
pub const PRETTY: &str = "Pretty";
pub const RAW: &str = "Raw";
pub const INVALID_JSON: &str = "Not valid JSON:";
pub const JSON_TRUNCATED: &str = "JSON cut at the preview limit; showing raw text.";

pub const COLLECTIONS: &str = "Collections";
pub const OPEN_REQUESTS: &str = "Unsaved requests";
pub const HISTORY: &str = "History";
pub const CLEAR_HISTORY: &str = "Clear history";
pub const IMPORT_CURL: &str = "Import cURL…";
pub const IMPORT_COLLECTION: &str = "Import collection…";
pub const EXPORT: &str = "Export…";
pub const EXPORT_WITH_SECRETS: &str = "Export including secrets…";
pub const DELETE: &str = "Delete";
pub const NO_FOLDER_HTTP: &str = "Open a folder to save collections and environments.";
pub const SAVE_REQUEST_TITLE: &str = "Save request";
pub const COLLECTION: &str = "Collection";
pub const NEW_COLLECTION_HINT: &str = "or a new collection name";
pub const IMPORT_CURL_TITLE: &str = "Import cURL";
pub const IMPORT_CURL_HELP: &str = "Paste one curl command (POSIX shell syntax). It is parsed, never executed. Unsupported options are reported.";
pub const IMPORT: &str = "Import";
pub const DELETE_COLLECTION_TITLE: &str = "Delete collection";
pub const DELETE_COLLECTION_BODY: &str = "This deletes the collection file from .simpleeditor/collections. Open request tabs keep their content as unsaved requests.";
pub const DELETE_REQUEST_TITLE: &str = "Delete request";
pub const DELETE_REQUEST_BODY: &str = "The request is removed from its collection file. An open tab keeps its content as an unsaved request.";

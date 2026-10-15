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

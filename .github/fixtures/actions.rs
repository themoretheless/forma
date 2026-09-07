// Compile-time contract fixture for the legacy Studio WebView host.
// The actual runner injects the open project's actions.rs instead.
pub struct SearchState {
    pub query: String,
    pub loading: bool,
    pub status: String,
}
pub fn search(state: &mut SearchState) {
    state.loading = false;
    state.status = format!("CI fixture: {}", state.query);
}

mod app;
mod events;
mod ui;

use std::io;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::blocking::Client;

use app::{AppState, BrowserApp, ModalState, PendingAction};
use crate::put;

pub fn run(client: &Client, api_token: &String) -> io::Result<()> {
    // Restore terminal on panic
    std::panic::set_hook(Box::new(|info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        eprintln!("{info}");
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = BrowserApp::new();

    loop {
        app.tick = app.tick.wrapping_add(1);
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if matches!(app.app_state, AppState::Quitting) {
            break;
        }

        // Draw happens before the load so the spinner is visible during the request.
        if app.needs_reload {
            app.needs_reload = false;
            load_current_folder(&mut app, client, api_token);
            continue;
        }

        // Handle pending actions that need the spinner frame to render first.
        let pending = std::mem::replace(&mut app.pending_action, PendingAction::None);
        match pending {
            PendingAction::None => {}

            PendingAction::Search { query } => {
                match put::files::search(client, api_token, &query) {
                    Ok(r) => app.enter_search_results(&query, r.files),
                    Err(e) => app.modal = ModalState::Error(format!("Search failed: {}", e)),
                }
            }

            PendingAction::GoToFolder { parent_id, file_id } => {
                app.navigate_to_folder(parent_id, file_id);
                app.needs_reload = true;
            }

            PendingAction::Download { file_id } => {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                terminal.show_cursor()?;

                match put::files::download(client, api_token, file_id, false, None, false) {
                    Ok(_) => {}
                    Err(e) => eprintln!("Download error: {}", e),
                }

                println!("\nPress Enter to return to the file browser...");
                let mut input = String::new();
                io::stdin().read_line(&mut input).ok();

                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            }
        }

        if matches!(app.app_state, AppState::Quitting) {
            break;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                events::handle_key(&mut app, key, client, api_token);
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

pub(super) fn load_current_folder(app: &mut BrowserApp, client: &Client, api_token: &String) {
    match put::files::list(client, api_token, app.current_folder_id) {
        Ok(r) => {
            // Update the breadcrumb name from the API response (covers "Go to folder" placeholders)
            if app.current_folder_id != 0 {
                if let Some(crumb) = app.breadcrumbs.last_mut() {
                    if crumb.id == app.current_folder_id {
                        crumb.name = r.parent.name.clone();
                    }
                }
            }
            app.set_files(r.files);
        }
        Err(e) => app.modal = ModalState::Error(e.to_string()),
    }
}

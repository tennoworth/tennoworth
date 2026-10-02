use image::RgbaImage;

pub(super) struct CapturedFrame {
    pub(super) image: RgbaImage,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// Which rule picked the captured window, named in the capture log.
#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowRule {
    GamePid,
    ExactTitle,
    TitleContains,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl WindowRule {
    fn describe(self) -> &'static str {
        match self {
            Self::GamePid => "game process",
            Self::ExactTitle => "exact title",
            Self::TitleContains => "title contains \"warframe\"",
        }
    }
}

/// The index of the game's window among `(pid, title)` candidates, listed in
/// the order the platform enumerates them.
///
/// Taking the first title containing "warframe" captured a browser on
/// warframe.market or a Discord channel whenever it was listed before the game.
/// The window owned by the scanner's game process is the game; failing that (no
/// game process, or a window manager that reports another pid, as Wine can), a
/// title of exactly "Warframe" is the game's own; the substring is the last
/// resort.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn pick_game_window(
    candidates: &[(Option<u32>, String)],
    game_pid: Option<u32>,
) -> Option<(usize, WindowRule)> {
    // The game can own more than one window (a launcher, a splash, hidden
    // helpers); among its own, the one titled "Warframe" is the one to read.
    let exact = |title: &str| title.trim().eq_ignore_ascii_case("warframe");
    let by_pid = game_pid.and_then(|game| {
        candidates
            .iter()
            .position(|(pid, title)| *pid == Some(game) && exact(title))
            .or_else(|| candidates.iter().position(|(pid, _)| *pid == Some(game)))
    });
    if let Some(index) = by_pid {
        return Some((index, WindowRule::GamePid));
    }
    if let Some(index) = candidates
        .iter()
        .position(|(_, title)| exact(title))
    {
        return Some((index, WindowRule::ExactTitle));
    }
    candidates
        .iter()
        .position(|(_, title)| title.to_ascii_lowercase().contains("warframe"))
        .map(|index| (index, WindowRule::TitleContains))
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::{pick_game_window, WindowRule};

    fn windows(list: &[(Option<u32>, &str)]) -> Vec<(Option<u32>, String)> {
        list.iter()
            .map(|(pid, title)| (*pid, (*title).to_string()))
            .collect()
    }

    #[test]
    fn the_game_s_own_titled_window_beats_its_other_windows() {
        let list = windows(&[
            (Some(42), "Warframe Launcher"),
            (Some(42), "Warframe"),
        ]);
        assert_eq!(pick_game_window(&list, Some(42)), Some((1, WindowRule::GamePid)));
    }

    #[test]
    fn a_browser_listed_first_does_not_beat_the_game_process() {
        let list = windows(&[
            (Some(10), "Warframe Market - Chrome"),
            (Some(42), "Warframe"),
        ]);
        assert_eq!(pick_game_window(&list, Some(42)), Some((1, WindowRule::GamePid)));
    }

    #[test]
    fn the_game_process_beats_an_exact_title_impostor() {
        let list = windows(&[(Some(10), "Warframe"), (Some(42), "Warframe ")]);
        assert_eq!(pick_game_window(&list, Some(42)), Some((1, WindowRule::GamePid)));
    }

    #[test]
    fn without_a_pid_match_the_exact_title_wins() {
        let list = windows(&[
            (Some(10), "Warframe Market - Chrome"),
            (None, " warframe "),
        ]);
        assert_eq!(pick_game_window(&list, None), Some((1, WindowRule::ExactTitle)));
        assert_eq!(pick_game_window(&list, Some(99)), Some((1, WindowRule::ExactTitle)));
    }

    #[test]
    fn the_substring_is_the_last_resort() {
        let list = windows(&[(Some(10), "Notes"), (Some(11), "Warframe (DX12)")]);
        assert_eq!(
            pick_game_window(&list, Some(99)),
            Some((1, WindowRule::TitleContains))
        );
    }

    #[test]
    fn no_candidate_matches_nothing() {
        let list = windows(&[(Some(10), "Notes"), (None, "")]);
        assert_eq!(pick_game_window(&list, Some(99)), None);
        assert_eq!(pick_game_window(&[], None), None);
    }
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(super) use windows::capture_warframe;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub(super) use x11::capture_warframe;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) use unsupported::capture_warframe;

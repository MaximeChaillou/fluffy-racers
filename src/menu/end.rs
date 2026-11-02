//! Issue #136: **the buttons of the end screens**, in place of the lines of keys (« Espace ou
//! Entrée (manette : A) : on rejoue »...). Max: « Quand on a fini une course on a plus de moyen de
//! revenir en arrière, on a juste une indication avec un raccourci pour rejouer. Je ne veux pas
//! d'indications avec des raccourcis, seulement des CTA clairs. »
//!
//! - **Which buttons** ([`EndScreen::buttons`], pure): the results of a circuit alone: « Rejouer »,
//!   « Changer de circuit » (« Jouer solo »), « Accueil »; of a time trial: « Réessayer »,
//!   « Changer de circuit », « Accueil »; in a championship, its results: « Classement général »,
//!   « Abandonner »; its standings: « Course suivante » (or « Le podium » after the last race),
//!   « Abandonner »; its podium: « Rejouer le championnat », « Accueil ». Online, the host moves
//!   the room on: « Course suivante » or « Le podium » in a championship, and « Retour au salon »
//!   (for everyone); a guest has « Retour au salon » (for itself, before the others) after a race
//!   alone and on the podium; between the races of a championship, the next one is the host's
//!   (its button is greyed, with when it comes). The automatic way back of #101 and #122 stays: its
//!   countdown is on the button. **From every end screen, a button goes home or to the lobby**
//!   (a guest between two races of a championship: the next race comes by itself; the pause's
//!   « Quitter » leaves).
//! - **Where**: in the slot the end screens leave for them under their panel (`kit::EndSlot`: the
//!   results of `ui`, the standings and the podium of `modes`), a row of the charter's buttons
//!   (a pill, its depth, its badge and icon, its label, its small line).
//! - **The keys**: Gauche / Droite (or Haut / Bas, the layout's letters too) move the focus,
//!   Entrée or Espace choose; the gamepad's cross or stick, A; the pointer gives the focus and a
//!   click chooses. One focus, as everywhere. Échap and Start open the pause, as before. They
//!   come [`END_INPUT_DELAY`] after the screen (a key still held from the race does nothing).
//!
//! **With a renderer only** (`MainMenuPlugin`): the headless world of the bot races has none of it
//! (its end screens have an empty slot); there, Entrée does what the first button does, as before
//! (`race`, `modes`). Its resource ([`EndMenu`]) comes with the first end screen; its systems only
//! run while an end screen is shown (or its row is still there).

use bevy::ecs::system::SystemParam;
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOver};
use bevy::prelude::*;
use bevy::text::Justify;

use super::anim;
use super::kit::{self, EndSlot};
use super::nav::{Entry, StickLatch};
use super::screen::{MenuAssets, MenuNode};
use super::style::{self, ButtonState, palette, u};
use super::{CONFIRM_KEYS, MainMenu};
use crate::characters::SelectionScreen;
use crate::kart::KeyboardLayout;
use crate::level::Online;
use crate::modes::{GrandPrixAction, RaceMenu, RaceMenuAction, TimeTrial};
use crate::net::NetSession;
use crate::options::OptionsMenu;
use crate::race::{GrandPrix, GrandPrixStage, Race, RacePhase};
use crate::salon::{SalonScreenRoot, SalonTuning};

/// The end screen's buttons take the keys this long after it showed (s, real time): a key or a
/// button still held from the race (the gamepad's A is the accelerator) does nothing. A constant
/// of the screens, like their animations.
pub const END_INPUT_DELAY: f64 = 0.5;

/// Sizes of the reference window (px): a button (its depth included), its label, its small line,
/// its badge and icon; the gap between two; the row's width for one, two or three buttons (three:
/// the results' panel).
pub mod end_size {
    pub const HEIGHT: f32 = 64.0;
    pub const LABEL: f32 = 20.0;
    pub const CAPTION: f32 = 14.0;
    pub const BADGE: f32 = 28.0;
    pub const ICON: f32 = 20.0;
    pub const GAP: f32 = 12.0;
    pub const PADDING: f32 = 10.0;
    pub const ROW: [f32; 3] = [300.0, 612.0, 720.0];
}

/// An end screen: what was raced, and where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndScreen {
    /// The results of a circuit alone (solo).
    Race,
    /// Issue #76: the results of a time trial.
    TimeTrial,
    /// The results of a race of a championship (solo).
    ChampionshipRace,
    /// The overall standings of a championship (solo); `last`: after its last race.
    Standings { last: bool },
    /// The podium of a championship (solo).
    Podium,
    /// Online (issues #9, #101): the results of a race alone; `host`: we host the room.
    OnlineRace { host: bool },
    /// Issue #122: online, the results of a race of the championship...
    OnlineChampionshipRace { host: bool },
    /// ...its overall standings...
    OnlineStandings { host: bool, last: bool },
    /// ...its podium.
    OnlinePodium { host: bool },
}

/// A button of an end screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndButton {
    /// « Rejouer »: the same race again, from its grid.
    Replay,
    /// « Réessayer »: the time trial again (against the ghost of the record).
    Retry,
    /// « Changer de circuit »: « Jouer solo » (issue #99).
    ChangeCircuit,
    /// « Accueil »: the home screen (issue #89).
    Home,
    /// « Classement général »: a championship's standings after its race.
    Standings,
    /// « Course suivante »: a championship's next race.
    NextRace,
    /// « Le podium »: after the last race.
    Podium,
    /// « Abandonner »: the championship stops, back home.
    GiveUp,
    /// « Rejouer le championnat »: the same races, from the first.
    ReplayChampionship,
    /// « Retour au salon »: the host brings everyone back; a guest goes before the others.
    BackToSalon,
}

impl EndButton {
    pub fn label(self) -> &'static str {
        match self {
            EndButton::Replay => "Rejouer",
            EndButton::Retry => "Réessayer",
            EndButton::ChangeCircuit => "Changer de circuit",
            EndButton::Home => "Accueil",
            EndButton::Standings => "Classement général",
            EndButton::NextRace => "Course suivante",
            EndButton::Podium => "Le podium",
            EndButton::GiveUp => "Abandonner",
            EndButton::ReplayChampionship => "Rejouer le championnat",
            EndButton::BackToSalon => "Retour au salon",
        }
    }

    /// The color of its badge: what goes on in Menthe (« Reprendre »), « Jouer solo »'s Lagon,
    /// home's Myrtille (« Quitter »), the lobby's Fraise (« Multijoueur »).
    pub fn accent(self) -> Color {
        match self {
            EndButton::Replay
            | EndButton::Retry
            | EndButton::Standings
            | EndButton::NextRace
            | EndButton::Podium
            | EndButton::ReplayChampionship => palette::MENTHE,
            EndButton::ChangeCircuit => palette::LAGON,
            EndButton::Home | EndButton::GiveUp => palette::MYRTILLE,
            EndButton::BackToSalon => palette::FRAISE,
        }
    }

    /// Its icon: the one of the menu's entry that does the same (again: « Recommencer »; on:
    /// « Reprendre »; « Jouer solo »; home: « Quitter »; « Multijoueur »).
    pub fn icon(self) -> Entry {
        match self {
            EndButton::Replay | EndButton::Retry | EndButton::ReplayChampionship => Entry::Restart,
            EndButton::Standings | EndButton::NextRace | EndButton::Podium => Entry::Resume,
            EndButton::ChangeCircuit => Entry::Solo,
            EndButton::Home | EndButton::GiveUp => Entry::Quit,
            EndButton::BackToSalon => Entry::Online,
        }
    }
}

impl EndScreen {
    /// Its buttons, the one that goes on first (it has the focus when the screen shows).
    pub fn buttons(self) -> &'static [EndButton] {
        use EndButton::*;
        match self {
            EndScreen::Race => &[Replay, ChangeCircuit, Home],
            EndScreen::TimeTrial => &[Retry, ChangeCircuit, Home],
            EndScreen::ChampionshipRace => &[Standings, GiveUp],
            EndScreen::Standings { last: false } => &[NextRace, GiveUp],
            EndScreen::Standings { last: true } => &[Podium, GiveUp],
            EndScreen::Podium => &[ReplayChampionship, Home],
            EndScreen::OnlineRace { .. } | EndScreen::OnlinePodium { .. } => &[BackToSalon],
            EndScreen::OnlineChampionshipRace { host: true } => &[Standings, BackToSalon],
            EndScreen::OnlineChampionshipRace { host: false } => &[Standings],
            EndScreen::OnlineStandings {
                host: true,
                last: false,
            } => &[NextRace, BackToSalon],
            EndScreen::OnlineStandings {
                host: true,
                last: true,
            } => &[Podium, BackToSalon],
            EndScreen::OnlineStandings {
                host: false,
                last: false,
            } => &[NextRace],
            EndScreen::OnlineStandings {
                host: false,
                last: true,
            } => &[Podium],
        }
    }

    /// Whether it is online.
    pub fn online(self) -> bool {
        matches!(
            self,
            EndScreen::OnlineRace { .. }
                | EndScreen::OnlineChampionshipRace { .. }
                | EndScreen::OnlineStandings { .. }
                | EndScreen::OnlinePodium { .. }
        )
    }

    /// We host the room (online).
    pub fn hosting(self) -> bool {
        matches!(
            self,
            EndScreen::OnlineRace { host: true }
                | EndScreen::OnlineChampionshipRace { host: true }
                | EndScreen::OnlineStandings { host: true, .. }
                | EndScreen::OnlinePodium { host: true }
        )
    }

    /// Online, how long the room waits before it moves on by itself (s), from when its countdown
    /// starts (a race alone: its table closed; the standings, the podium: shown): back to the
    /// lobby after a race alone (#101), the next race or the podium after the standings, the
    /// lobby after the podium (#122). The host's `SalonTuning`, the same everywhere.
    pub fn countdown(self, tuning: &SalonTuning) -> Option<f32> {
        match self {
            EndScreen::OnlineRace { .. } => Some(tuning.back_after_results),
            EndScreen::OnlineStandings { .. } => Some(tuning.standings_seconds),
            EndScreen::OnlinePodium { .. } => Some(tuning.podium_seconds),
            _ => None,
        }
    }
}

/// What the game says of the end of the race: the end screen it shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EndFacts {
    /// The race's results are on (its phase).
    pub results: bool,
    /// They take a choice (solo: `RaceFlow::can_restart`: a restart would be honoured).
    pub ready: bool,
    /// A championship: its stage, and whether its race is the last.
    pub championship: Option<(GrandPrixStage, bool)>,
    /// Issue #76: a time trial.
    pub time_trial: bool,
    /// Online: whether we host the room.
    pub online: Option<bool>,
}

/// The end screen shown, if any.
pub fn end_screen(facts: EndFacts) -> Option<EndScreen> {
    if !facts.results {
        return None;
    }
    if let Some(host) = facts.online {
        return Some(match facts.championship {
            None => EndScreen::OnlineRace { host },
            Some((GrandPrixStage::Racing, _)) => EndScreen::OnlineChampionshipRace { host },
            Some((GrandPrixStage::Standings, last)) => EndScreen::OnlineStandings { host, last },
            Some((GrandPrixStage::Podium, _)) => EndScreen::OnlinePodium { host },
        });
    }
    if !facts.ready {
        return None;
    }
    Some(match facts.championship {
        Some((GrandPrixStage::Racing, _)) => EndScreen::ChampionshipRace,
        Some((GrandPrixStage::Standings, last)) => EndScreen::Standings { last },
        Some((GrandPrixStage::Podium, _)) => EndScreen::Podium,
        None if facts.time_trial => EndScreen::TimeTrial,
        None => EndScreen::Race,
    })
}

/// What the small lines say besides the screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndContext {
    /// A championship's next race: its number, the races of the championship, its circuit.
    pub next: Option<(usize, usize, String)>,
    /// Online: the seconds before the room moves on by itself, once its countdown started.
    pub seconds: Option<u32>,
    /// Online, a race alone: its table closed (everyone finished, or the time ran out).
    pub closed: bool,
}

/// Whether `button` of `screen` can be used now. Online, a guest waits for the host between the
/// races of a championship; nobody closes the table before everyone finished (the host's rules,
/// #9), so the host sends everyone back once it closed.
pub fn usable(screen: EndScreen, button: EndButton, context: &EndContext) -> bool {
    match (screen, button) {
        (EndScreen::OnlineChampionshipRace { .. }, EndButton::Standings) => false,
        (EndScreen::OnlineStandings { host: false, .. }, _) => false,
        (EndScreen::OnlineRace { host: true }, EndButton::BackToSalon) => context.closed,
        _ => true,
    }
}

/// The small line under `button`: what it does (« Rejouer »: « La même course, chrono à zéro. »),
/// online when (« Dans 5 s, pour tout le monde. »); when it cannot be used, why.
pub fn caption(screen: EndScreen, button: EndButton, context: &EndContext) -> String {
    let host = screen.hosting();
    let seconds = context.seconds;
    match button {
        EndButton::Replay => "La même course, chrono à zéro.".to_string(),
        EndButton::Retry => "Le même circuit, chrono à zéro.".to_string(),
        EndButton::ChangeCircuit => "Un autre circuit, un autre mode.".to_string(),
        EndButton::Home => "Retour à l'accueil.".to_string(),
        EndButton::GiveUp => "Le championnat s'arrête là.".to_string(),
        EndButton::ReplayChampionship => "Les mêmes courses, depuis la première.".to_string(),
        EndButton::Standings if screen.online() => "Quand tout le monde a fini.".to_string(),
        EndButton::Standings => "Les points de cette course.".to_string(),
        EndButton::NextRace | EndButton::Podium if screen.online() && !host => match seconds {
            Some(s) => format!("Dans {s} s : l'hôte décide."),
            None => "C'est l'hôte qui décide.".to_string(),
        },
        EndButton::NextRace => match (&context.next, seconds) {
            (Some((_, _, name)), Some(s)) => format!("{name}, dans {s} s"),
            (Some((race, races, name)), None) => format!("Course {race}/{races} : {name}"),
            (None, _) => String::new(),
        },
        EndButton::Podium => match seconds {
            Some(s) if screen.online() => format!("Dans {s} s, pour tout le monde."),
            _ if screen.online() => "Pour tout le monde.".to_string(),
            _ => "Les trois premiers sur les marches.".to_string(),
        },
        EndButton::BackToSalon => match (screen, seconds) {
            (EndScreen::OnlineRace { host: true }, _) if !context.closed => {
                "Quand tout le monde a fini.".to_string()
            }
            (EndScreen::OnlineChampionshipRace { .. } | EndScreen::OnlineStandings { .. }, _) => {
                "Le championnat s'arrête pour tous.".to_string()
            }
            (_, Some(s)) if host => format!("Dans {s} s, pour tout le monde."),
            (_, None) if host => "Pour tout le monde.".to_string(),
            (_, Some(s)) => format!("Dans {s} s, avec tout le monde."),
            (_, None) => "Tu y vas avant les autres.".to_string(),
        },
    }
}

/// The focus `step` buttons away from `focus` (`step` < 0: left), around, over those that cannot
/// be used. No focus yet: the first usable one that way. None usable: none.
pub fn moved(usable: &[bool], focus: Option<usize>, step: i32) -> Option<usize> {
    let count = usable.len() as i32;
    if count == 0 || !usable.contains(&true) {
        return None;
    }
    let direction = step.signum();
    let mut at = match focus {
        Some(focus) => focus as i32,
        // From « before the first » (right) or « after the last » (left).
        None if direction < 0 => count,
        None => -1,
    };
    if direction == 0 {
        return focus.filter(|f| usable.get(*f).copied().unwrap_or(false));
    }
    for _ in 0..count {
        at = (at + direction).rem_euclid(count);
        if usable[at as usize] {
            return Some(at as usize);
        }
    }
    focus
}

/// The focus when the buttons show: on the first one, the one that goes on, if it can be used;
/// otherwise none (online, the host's first button may be the guest's to wait for: a key does
/// not send the room back by surprise).
pub fn first_focus(usable: &[bool]) -> Option<usize> {
    usable.first().copied().unwrap_or(false).then_some(0)
}

/// The most buttons a screen has.
const MOST: usize = 3;

/// The row of buttons as built: its screen, its slot, which can be used. Another, another row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EndShape {
    screen: EndScreen,
    slot: Entity,
    usable: [bool; MOST],
}

/// The end screen's buttons: what is shown, the focus, the countdown. There from the first end
/// screen on.
#[derive(Resource, Debug, Default)]
pub struct EndMenu {
    /// The end screen shown.
    pub screen: Option<EndScreen>,
    /// The button with the focus (an index of its screen's buttons), if any.
    pub focus: Option<usize>,
    /// When the screen showed, when the focus last moved (real time, s), and which lost it.
    pub shown_at: f64,
    pub focused_at: f64,
    pub unfocused: Option<usize>,
    /// Online: what the host chose that moves the room (its next race, the podium, the way
    /// back), taken by `salon` (it runs the room: `EndMenu::take`).
    pub chosen: Option<EndButton>,
    /// Online, a race alone: when its table closed (the countdown starts then).
    closed_at: Option<f64>,
    /// The button the pointer clicked: chosen with the keys' on the next run.
    clicked: Option<usize>,
    /// The row built, and the seconds its small lines say.
    shape: Option<EndShape>,
    shown_seconds: Option<u32>,
    /// The gamepad's stick, as steps (horizontal, vertical).
    latches: [StickLatch; 2],
    /// Another screen had the keys on the last frame (the key that closed it does nothing here).
    held: bool,
}

impl EndMenu {
    /// Takes what the host chose, if `wanted`: `salon` moves the room on with it.
    pub fn take(&mut self, wanted: impl Fn(EndButton) -> bool) -> Option<EndButton> {
        self.chosen.take_if(|button| wanted(*button))
    }

    /// Which of the screen's buttons can be used, as built.
    pub fn usable(&self) -> &[bool] {
        match (&self.shape, self.screen) {
            (Some(shape), Some(screen)) => &shape.usable[..screen.buttons().len().min(MOST)],
            _ => &[],
        }
    }
}

/// The row of buttons, in its slot.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct EndRow;

/// A button of the row: its index and what it does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct EndButtonNode {
    pub index: usize,
    pub button: EndButton,
}

/// A button's badge.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct EndBadge {
    pub index: usize,
}

/// A button's label, or its small line (`caption`).
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct EndText {
    pub index: usize,
    pub caption: bool,
}

/// What the game says of the end of the race (see [`EndFacts`]) and of what comes next.
#[derive(SystemParam)]
pub(super) struct EndState<'w> {
    race: Option<Res<'w, Race>>,
    grand_prix: Option<Res<'w, GrandPrix>>,
    time_trial: Option<Res<'w, TimeTrial>>,
    online: Option<Res<'w, Online>>,
    session: Option<Res<'w, NetSession>>,
    salon: Option<Res<'w, SalonTuning>>,
}

impl EndState<'_> {
    fn facts(&self) -> EndFacts {
        let Some(race) = self.race.as_deref() else {
            return EndFacts::default();
        };
        let championship = self.grand_prix.as_deref().map(|gp| {
            let last = gp.cup.get(gp.race + 1).is_none();
            (gp.stage, last)
        });
        let online = self.online.is_some().then(|| {
            self.grand_prix
                .as_deref()
                .and_then(|gp| gp.online.as_ref())
                .map(|online| online.hosting)
                // Issue #225: the leader of a dedicated server's game has the host's buttons.
                .or(self.session.as_deref().map(NetSession::leads))
                .unwrap_or(false)
        });
        EndFacts {
            results: race.phase() == RacePhase::Results,
            ready: race.flow.can_restart(&race.timings),
            championship,
            time_trial: self.time_trial.is_some(),
            online,
        }
    }

    /// A race alone online: its table is closed.
    fn closed(&self) -> bool {
        self.race.as_deref().is_some_and(Race::results_closed)
    }

    /// A championship's next race: its number, of all, its circuit.
    fn next(&self) -> Option<(usize, usize, String)> {
        let gp = self.grand_prix.as_deref()?;
        let next = gp.cup.get(gp.race + 1)?;
        Some((gp.race + 2, gp.cup.len(), next.name.clone()))
    }
}

/// The end screen's systems run: one is shown, or its row is still there (to go).
pub(super) fn end_wanted_or_shown(race: Option<Res<Race>>, menu: Option<Res<EndMenu>>) -> bool {
    race.is_some_and(|race| race.phase() == RacePhase::Results)
        || menu.is_some_and(|menu| menu.screen.is_some() || menu.shape.is_some())
}

/// The end screen of now: its row of buttons in its slot (built again when the screen, the slot
/// or what can be used changes), its small lines following the countdown online.
#[allow(clippy::too_many_arguments)]
pub(super) fn follow_the_end(
    mut commands: Commands,
    state: EndState,
    menu: Option<ResMut<EndMenu>>,
    slots: Query<Entity, With<EndSlot>>,
    rows: Query<Entity, With<EndRow>>,
    assets: Option<Res<MenuAssets>>,
    time: Res<Time<Real>>,
    mut texts: Query<(&EndText, &mut Text)>,
) {
    let now = time.elapsed_secs_f64();
    let screen = end_screen(state.facts());
    let Some(mut menu) = menu else {
        if screen.is_some() {
            commands.insert_resource(EndMenu::default());
        }
        return;
    };
    if menu.screen != screen {
        menu.screen = screen;
        menu.shown_at = now;
        menu.focused_at = now - f64::from(anim::FOCUS_TIME);
        menu.unfocused = None;
        menu.focus = None;
        menu.closed_at = None;
        menu.clicked = None;
        menu.chosen = None;
        menu.shown_seconds = None;
    }
    let (Some(screen), Some(slot)) = (screen, slots.iter().next()) else {
        for row in &rows {
            commands.entity(row).despawn();
        }
        menu.shape = None;
        return;
    };
    // Online, a race alone: its countdown starts when its table closes.
    let closed = state.closed();
    if closed && menu.closed_at.is_none() {
        menu.closed_at = Some(now);
    }
    let from = match screen {
        EndScreen::OnlineRace { .. } => menu.closed_at,
        _ => Some(menu.shown_at),
    };
    let seconds = state
        .salon
        .as_deref()
        .and_then(|tuning| screen.countdown(tuning))
        .zip(from)
        .map(|(wait, from)| (f64::from(wait.max(0.0)) - (now - from)).max(0.0).ceil() as u32);
    // The next race's name only when a small line is written (no string each frame).
    let mut context = EndContext {
        next: None,
        seconds,
        closed,
    };
    let buttons = screen.buttons();
    let mut usable_now = [false; MOST];
    for (slot_usable, button) in usable_now.iter_mut().zip(buttons) {
        *slot_usable = usable(screen, *button, &context);
    }
    let shape = EndShape {
        screen,
        slot,
        usable: usable_now,
    };
    if menu.shape != Some(shape) {
        for row in &rows {
            commands.entity(row).despawn();
        }
        let usable = &usable_now[..buttons.len().min(MOST)];
        if menu
            .focus
            .is_none_or(|focus| !usable.get(focus).copied().unwrap_or(false))
        {
            menu.focus = first_focus(usable);
        }
        let assets = assets.map(|assets| assets.clone()).unwrap_or_default();
        context.next = state.next();
        spawn_row(&mut commands, slot, screen, &context, &menu, &assets);
        menu.shape = Some(shape);
        menu.shown_seconds = seconds;
        return;
    }
    // The countdown: only its small lines change, once a second.
    if menu.shown_seconds != seconds {
        menu.shown_seconds = seconds;
        context.next = state.next();
        for (text, mut written) in &mut texts {
            let Some(button) = buttons.get(text.index).filter(|_| text.caption) else {
                continue;
            };
            let wanted = caption(screen, *button, &context);
            if written.0 != wanted {
                written.0 = wanted;
            }
        }
    }
}

/// The row of `screen`'s buttons in `slot`.
fn spawn_row(
    commands: &mut Commands,
    slot: Entity,
    screen: EndScreen,
    context: &EndContext,
    menu: &EndMenu,
    assets: &MenuAssets,
) {
    let buttons = screen.buttons();
    let width = end_size::ROW[buttons.len().clamp(1, MOST) - 1];
    let row = commands
        .spawn((
            Name::new("Fin de course : boutons"),
            EndRow,
            Node {
                width: u(width),
                column_gap: u(end_size::GAP),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            for (index, button) in buttons.iter().copied().enumerate() {
                let usable = usable(screen, button, context);
                spawn_button(
                    row,
                    index,
                    button,
                    usable,
                    menu.focus == Some(index),
                    caption(screen, button, context),
                    assets,
                );
            }
        })
        .id();
    commands.entity(slot).add_child(row);
}

/// The state of a button: grey when it cannot be used, Mangue with the focus.
fn state_of(usable: bool, focused: bool) -> ButtonState {
    match (usable, focused) {
        (false, _) => ButtonState::Disabled,
        (true, true) => ButtonState::Focused,
        (true, false) => ButtonState::Normal,
    }
}

/// A button: a pill with its depth, its badge and icon and its label on a line, its small line
/// under them.
fn spawn_button(
    row: &mut ChildSpawnerCommands,
    index: usize,
    button: EndButton,
    usable: bool,
    focused: bool,
    caption: String,
    assets: &MenuAssets,
) {
    let look = style::button_look(state_of(usable, focused), button.accent());
    let mut node = row.spawn((
        Name::new(button.label()),
        EndButtonNode { index, button },
        Node {
            flex_grow: 1.0,
            flex_basis: Val::ZERO,
            min_height: u(end_size::HEIGHT),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: u(2.0),
            padding: UiRect::axes(u(end_size::PADDING), u(4.0)),
            border: UiRect::bottom(u(look.depth_px)),
            border_radius: BorderRadius::all(u(end_size::HEIGHT / 2.0)),
            ..default()
        },
        BackgroundColor(look.fill),
        BorderColor::all(look.depth),
        UiTransform::from_scale(Vec2::splat(anim::BUTTON_POP_FROM)),
    ));
    node.with_children(|node| {
        node.spawn((
            MenuNode,
            Node {
                column_gap: u(8.0),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|top| {
            top.spawn((
                EndBadge { index },
                Node {
                    width: u(end_size::BADGE),
                    height: u(end_size::BADGE),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(look.badge),
                Pickable::IGNORE,
            ))
            .with_children(|badge| {
                badge.spawn((
                    MenuNode,
                    ImageNode::new(assets.icon(button.icon())),
                    Node {
                        width: u(end_size::ICON),
                        height: u(end_size::ICON),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            });
            top.spawn((
                EndText {
                    index,
                    caption: false,
                },
                kit::text(
                    button.label(),
                    &assets.semibold,
                    end_size::LABEL,
                    look.label,
                ),
                Pickable::IGNORE,
            ));
        });
        node.spawn((
            EndText {
                index,
                caption: true,
            },
            kit::text(caption, &assets.medium, end_size::CAPTION, look.caption),
            TextLayout::justify(Justify::Center),
            Pickable::IGNORE,
        ));
    });
    if usable {
        node.observe(
            move |_: On<PointerOver>, menu: Option<ResMut<EndMenu>>, time: Res<Time<Real>>| {
                if let Some(mut menu) = menu {
                    set_focus(&mut menu, Some(index), time.elapsed_secs_f64());
                }
            },
        )
        .observe(move |_: On<PointerClick>, menu: Option<ResMut<EndMenu>>| {
            if let Some(mut menu) = menu {
                menu.clicked = Some(index);
            }
        });
    }
}

fn set_focus(menu: &mut EndMenu, focus: Option<usize>, now: f64) {
    if menu.focus != focus {
        menu.unfocused = menu.focus;
        menu.focus = focus;
        menu.focused_at = now;
    }
}

/// The screens that take the keys over an end screen.
#[derive(SystemParam)]
pub(super) struct OverTheEnd<'w, 's> {
    options: Option<Res<'w, OptionsMenu>>,
    race_menu: Option<Res<'w, RaceMenu>>,
    selection: Option<Res<'w, SelectionScreen>>,
    capture: Option<Res<'w, crate::admin::KeyboardCapture>>,
    salon: Query<'w, 's, (), With<SalonScreenRoot>>,
}

impl OverTheEnd<'_, '_> {
    /// Whether one has the keys (the main menu: `main`).
    fn any(&self, main: &MainMenu) -> bool {
        main.open
            || main.handing_over.is_some()
            || self.options.as_ref().is_some_and(|o| o.open)
            || self.race_menu.as_ref().is_some_and(|r| r.open)
            || self.selection.as_ref().is_some_and(|s| s.open)
            || self.capture.as_ref().is_some_and(|c| c.0)
            || !self.salon.is_empty()
    }
}

/// What the end screen's buttons do.
#[derive(SystemParam)]
pub(super) struct EndDoers<'w> {
    race: Option<ResMut<'w, Race>>,
    session: Option<ResMut<'w, NetSession>>,
    championship: MessageWriter<'w, GrandPrixAction>,
    race_menu: MessageWriter<'w, RaceMenuAction>,
}

/// The keys, the gamepad and the pointer's click on the end screen's buttons (see the module's
/// documentation), and what the chosen one does: the race again (`Race::restart_requested`: from
/// its grid), « Jouer solo », the home screen (its background replaces the race; a championship
/// or a time trial ends), the championship moved on (`GrandPrixAction`); online, the host's
/// choice for `salon` ([`EndMenu::chosen`]), a guest's way back to the lobby
/// (`net::Lobby::leave_results`: the lobby's scene for it alone, `net::online`).
#[allow(clippy::too_many_arguments)]
pub(super) fn end_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    layout: Option<Res<KeyboardLayout>>,
    time: Res<Time<Real>>,
    menu: Option<ResMut<EndMenu>>,
    over: OverTheEnd,
    mut main: ResMut<MainMenu>,
    mut doers: EndDoers,
) {
    let Some(mut menu) = menu else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let clicked = menu.clicked.take();
    let (Some(screen), true) = (menu.screen, menu.shape.is_some()) else {
        return;
    };
    // Another screen has the keys, or had them on the last frame (the key that closed it).
    let held = over.any(&main);
    let was_held = std::mem::replace(&mut menu.held, held);
    if held || was_held || now - menu.shown_at < END_INPUT_DELAY {
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let key = |set: &[KeyCode]| {
        keys.as_ref()
            .is_some_and(|keys| keys.any_just_pressed(set.iter().copied()))
    };
    let pad = |button: GamepadButton| gamepads.iter().any(|g| g.just_pressed(button));
    let (x, y) = gamepads.iter().fold((0.0f32, 0.0f32), |(x, y), gamepad| {
        let stick = |axis| gamepad.get(axis).unwrap_or(0.0);
        let (sx, sy) = (
            stick(GamepadAxis::LeftStickX),
            stick(GamepadAxis::LeftStickY),
        );
        (
            if sx.abs() > x.abs() { sx } else { x },
            if sy.abs() > y.abs() { sy } else { y },
        )
    });
    // The stick: right is a step forwards (`StickLatch` counts up as -1), down too.
    let (across, down) = (-menu.latches[0].step(x), menu.latches[1].step(y));
    let back = key(layout.left_keys())
        || key(layout.up_keys())
        || pad(GamepadButton::DPadLeft)
        || pad(GamepadButton::DPadUp)
        || across < 0
        || down < 0;
    let on = key(layout.right_keys())
        || key(layout.down_keys())
        || pad(GamepadButton::DPadRight)
        || pad(GamepadButton::DPadDown)
        || across > 0
        || down > 0;
    // The buttons that can be used (copied: the focus moves below).
    let mut usable = [false; MOST];
    let count = menu.usable().len();
    usable[..count].copy_from_slice(menu.usable());
    let usable = &usable[..count];
    if back != on {
        let focus = moved(usable, menu.focus, if back { -1 } else { 1 });
        set_focus(&mut menu, focus, now);
    }
    let confirmed = key(&CONFIRM_KEYS) || pad(GamepadButton::South);
    let chosen = clicked
        .or(confirmed.then_some(menu.focus).flatten())
        .filter(|index| usable.get(*index).copied().unwrap_or(false));
    let Some(button) = chosen.and_then(|index| screen.buttons().get(index).copied()) else {
        return;
    };
    info!("end screen: « {} »", button.label());
    match button {
        EndButton::Replay | EndButton::Retry => {
            if let Some(race) = doers.race.as_mut() {
                race.restart_requested = true;
            }
        }
        EndButton::ChangeCircuit => {
            doers.race_menu.write(RaceMenuAction::Toggle);
        }
        EndButton::Home | EndButton::GiveUp => main.show(false, None, false, now),
        EndButton::Standings
        | EndButton::NextRace
        | EndButton::Podium
        | EndButton::ReplayChampionship => {
            if screen.online() {
                menu.chosen = Some(button);
            } else {
                doers.championship.write(GrandPrixAction::Continue);
            }
        }
        EndButton::BackToSalon if screen.hosting() => menu.chosen = Some(button),
        EndButton::BackToSalon => {
            if let Some(lobby) = doers.session.as_mut().and_then(|s| s.lobby.as_mut()) {
                lobby.leave_results();
            }
        }
    }
}

/// The buttons shown: each one takes its state's look (the focus in Mangue, grey when it cannot
/// be used) and its pose (they pop in one after the other, the focus grows and breathes). Only
/// what changed is written.
#[allow(clippy::type_complexity)]
pub(super) fn animate_end(
    menu: Option<Res<EndMenu>>,
    time: Res<Time<Real>>,
    mut buttons: Query<(
        &EndButtonNode,
        &mut UiTransform,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut badges: Query<(&EndBadge, &mut BackgroundColor), Without<EndButtonNode>>,
    mut texts: Query<(&EndText, &mut TextColor)>,
) {
    let Some(menu) = menu else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let since = |at: f64| (now - at) as f32;
    let usable = menu.usable();
    let look_of = |index: usize, button: EndButton| {
        let usable = usable.get(index).copied().unwrap_or(false);
        style::button_look(state_of(usable, menu.focus == Some(index)), button.accent())
    };
    let Some(screen) = menu.screen else {
        return;
    };
    let accent_of = |index: usize| screen.buttons().get(index).copied();
    for (node, mut transform, mut fill, mut depth) in &mut buttons {
        let look = look_of(node.index, node.button);
        let focus = if menu.focus == Some(node.index) {
            anim::focus_scale(true, since(menu.focused_at))
        } else if menu.unfocused == Some(node.index) {
            anim::focus_scale(false, since(menu.focused_at))
        } else {
            1.0
        };
        let pop = anim::button_pop(since(menu.shown_at), node.index);
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(pop * focus)));
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
    }
    for (badge, mut fill) in &mut badges {
        if let Some(button) = accent_of(badge.index) {
            fill.set_if_neq(BackgroundColor(look_of(badge.index, button).badge));
        }
    }
    for (text, mut color) in &mut texts {
        if let Some(button) = accent_of(text.index) {
            let look = look_of(text.index, button);
            color.set_if_neq(TextColor(if text.caption {
                look.caption
            } else {
                look.label
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #136: each end screen has its buttons, the one that goes on first; every one of
    /// them goes home or to the lobby, except a guest's between two races of a championship (the
    /// next one comes by itself); the labels are the issue's, short enough for their row.
    #[test]
    fn each_end_screen_has_its_buttons_and_a_way_back() {
        use EndButton::*;
        let screens = [
            (EndScreen::Race, vec![Replay, ChangeCircuit, Home]),
            (EndScreen::TimeTrial, vec![Retry, ChangeCircuit, Home]),
            (EndScreen::ChampionshipRace, vec![Standings, GiveUp]),
            (EndScreen::Standings { last: false }, vec![NextRace, GiveUp]),
            (EndScreen::Standings { last: true }, vec![Podium, GiveUp]),
            (EndScreen::Podium, vec![ReplayChampionship, Home]),
            (EndScreen::OnlineRace { host: true }, vec![BackToSalon]),
            (EndScreen::OnlineRace { host: false }, vec![BackToSalon]),
            (
                EndScreen::OnlineChampionshipRace { host: true },
                vec![Standings, BackToSalon],
            ),
            (
                EndScreen::OnlineStandings {
                    host: true,
                    last: false,
                },
                vec![NextRace, BackToSalon],
            ),
            (
                EndScreen::OnlineStandings {
                    host: true,
                    last: true,
                },
                vec![Podium, BackToSalon],
            ),
            (EndScreen::OnlinePodium { host: true }, vec![BackToSalon]),
            (EndScreen::OnlinePodium { host: false }, vec![BackToSalon]),
        ];
        for (screen, buttons) in &screens {
            assert_eq!(screen.buttons(), buttons.as_slice(), "{screen:?}");
            assert!(
                buttons
                    .iter()
                    .any(|b| matches!(b, Home | GiveUp | BackToSalon)),
                "{screen:?}: a way back"
            );
            assert!(buttons.len() <= MOST);
        }
        // A guest between two races of a championship: the host's next race, greyed.
        for last in [false, true] {
            let screen = EndScreen::OnlineStandings { host: false, last };
            let context = EndContext {
                seconds: Some(5),
                ..default()
            };
            assert_eq!(screen.buttons().len(), 1);
            assert!(!usable(screen, screen.buttons()[0], &context));
            assert_eq!(
                caption(screen, screen.buttons()[0], &context),
                "Dans 5 s : l'hôte décide."
            );
        }
        let guest = EndScreen::OnlineChampionshipRace { host: false };
        assert_eq!(guest.buttons(), [Standings]);
        assert!(!usable(guest, Standings, &EndContext::default()));
        for button in [
            Replay,
            Retry,
            ChangeCircuit,
            Home,
            Standings,
            NextRace,
            Podium,
            GiveUp,
            ReplayChampionship,
            BackToSalon,
        ] {
            assert!(button.label().chars().count() <= 22, "{}", button.label());
        }
        assert_eq!(Replay.label(), "Rejouer");
        assert_eq!(ChangeCircuit.label(), "Changer de circuit");
        assert_eq!(Home.label(), "Accueil");
        assert_eq!(ReplayChampionship.label(), "Rejouer le championnat");
        assert_eq!(BackToSalon.label(), "Retour au salon");
    }

    /// The end screen shown: solo once its results take a choice; a championship's stage; a time
    /// trial; online, as soon as the results are on, the host's or a guest's.
    #[test]
    fn the_end_screen_follows_the_race() {
        let mut facts = EndFacts::default();
        assert_eq!(end_screen(facts), None, "racing");
        facts.results = true;
        assert_eq!(end_screen(facts), None, "not yet: a key held from the race");
        facts.ready = true;
        assert_eq!(end_screen(facts), Some(EndScreen::Race));
        facts.time_trial = true;
        assert_eq!(end_screen(facts), Some(EndScreen::TimeTrial));
        facts.time_trial = false;
        facts.championship = Some((GrandPrixStage::Racing, false));
        assert_eq!(end_screen(facts), Some(EndScreen::ChampionshipRace));
        facts.championship = Some((GrandPrixStage::Standings, true));
        assert_eq!(end_screen(facts), Some(EndScreen::Standings { last: true }));
        facts.championship = Some((GrandPrixStage::Podium, true));
        assert_eq!(end_screen(facts), Some(EndScreen::Podium));
        // Online: as soon as the results are on (the host closes the table).
        let online = EndFacts {
            results: true,
            online: Some(false),
            ..default()
        };
        assert_eq!(
            end_screen(online),
            Some(EndScreen::OnlineRace { host: false })
        );
        let championship = EndFacts {
            championship: Some((GrandPrixStage::Standings, false)),
            online: Some(true),
            ..online
        };
        assert_eq!(
            end_screen(championship),
            Some(EndScreen::OnlineStandings {
                host: true,
                last: false
            })
        );
    }

    /// The small lines: what each button does, the next race, the countdown online (the host's:
    /// for everyone; a guest's: with everyone), the host's way back only once the table closed.
    #[test]
    fn the_small_lines_say_what_and_when() {
        let solo = EndContext {
            next: Some((3, 4, "Crique Câline".to_string())),
            ..default()
        };
        let standings = EndScreen::Standings { last: false };
        assert_eq!(
            caption(standings, EndButton::NextRace, &solo),
            "Course 3/4 : Crique Câline"
        );
        let host = EndScreen::OnlineStandings {
            host: true,
            last: false,
        };
        let soon = EndContext {
            seconds: Some(5),
            ..solo.clone()
        };
        assert_eq!(
            caption(host, EndButton::NextRace, &soon),
            "Crique Câline, dans 5 s"
        );
        assert_eq!(
            caption(host, EndButton::BackToSalon, &soon),
            "Le championnat s'arrête pour tous."
        );
        let race = EndScreen::OnlineRace { host: true };
        let open = EndContext::default();
        assert!(!usable(race, EndButton::BackToSalon, &open));
        assert_eq!(
            caption(race, EndButton::BackToSalon, &open),
            "Quand tout le monde a fini."
        );
        let closed = EndContext {
            closed: true,
            seconds: Some(6),
            ..default()
        };
        assert!(usable(race, EndButton::BackToSalon, &closed));
        assert_eq!(
            caption(race, EndButton::BackToSalon, &closed),
            "Dans 6 s, pour tout le monde."
        );
        let guest = EndScreen::OnlineRace { host: false };
        assert!(
            usable(guest, EndButton::BackToSalon, &open),
            "before the others"
        );
        assert_eq!(
            caption(guest, EndButton::BackToSalon, &open),
            "Tu y vas avant les autres."
        );
        assert_eq!(
            caption(guest, EndButton::BackToSalon, &closed),
            "Dans 6 s, avec tout le monde."
        );
        // Every small line is short, and none names a key.
        let contexts = [open, closed, solo, soon];
        for screen in [
            EndScreen::Race,
            EndScreen::TimeTrial,
            EndScreen::ChampionshipRace,
            EndScreen::Standings { last: false },
            EndScreen::Standings { last: true },
            EndScreen::Podium,
            EndScreen::OnlineRace { host: true },
            EndScreen::OnlineRace { host: false },
            EndScreen::OnlineChampionshipRace { host: true },
            EndScreen::OnlineChampionshipRace { host: false },
            EndScreen::OnlineStandings {
                host: false,
                last: true,
            },
            EndScreen::OnlinePodium { host: true },
            EndScreen::OnlinePodium { host: false },
        ] {
            for button in screen.buttons() {
                for context in &contexts {
                    let line = caption(screen, *button, context);
                    assert!(line.chars().count() < 48, "{line}");
                    assert!(!crate::ui::format::names_a_key(&line), "{line}");
                }
            }
        }
    }

    /// The focus goes around, over the buttons that cannot be used; none at first when the first
    /// one cannot be used (online: no key sends the room back by surprise).
    #[test]
    fn the_focus_goes_around_the_usable_buttons() {
        assert_eq!(first_focus(&[true, true, true]), Some(0));
        assert_eq!(first_focus(&[false, true]), None);
        assert_eq!(moved(&[true, true, true], Some(0), 1), Some(1));
        assert_eq!(moved(&[true, true, true], Some(2), 1), Some(0), "around");
        assert_eq!(moved(&[true, true, true], Some(0), -1), Some(2), "around");
        assert_eq!(moved(&[false, true], None, 1), Some(1));
        assert_eq!(moved(&[false, true], None, -1), Some(1));
        assert_eq!(moved(&[false, true], Some(1), 1), Some(1), "nothing else");
        assert_eq!(moved(&[false], None, 1), None);
        assert_eq!(moved(&[], None, 1), None);
    }
}

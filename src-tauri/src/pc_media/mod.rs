//! Pause music and videos on this PC while the iPhone rings (Settings › iPhone › Calls), and play
//! them again afterwards. Windows lists whatever is playing as media sessions (Spotify, a browser
//! tab, a video player: `GlobalSystemMediaTransportControlsSessionManager`); tug pauses the ones
//! playing, keeps hold of exactly those, and later plays each again only if it's still as tug
//! left it (`policy`). *When* is the window's decision (`src/lib/callPause.ts`), because the
//! ringing call lives there.
//!
//! Every Windows call runs on a time-limited helper (`ble::winrt::off_thread`), so a media app
//! that doesn't answer can't hold a command. Nothing about what's playing (titles, artists) is
//! logged, only which apps.

// Used only by the Windows side (and the tests).
#[cfg_attr(not(windows), allow(dead_code))]
pub mod policy;

use std::sync::{Mutex, MutexGuard};

/// The sessions tug paused and hasn't played again or let go of yet.
static HELD: Mutex<Vec<io::Held>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<io::Held>> {
    HELD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Pause what's playing on this PC. Answers how many sessions tug now holds paused, counting any
/// from an earlier ring it hasn't played again yet (a second call while the first is answered).
pub async fn pause() -> usize {
    let paused = io::pause(crate::quick_actions::own_app_id()).await;
    let mut held = held();
    held.extend(paused);
    held.len()
}

/// Play again what tug paused, where it's still as tug left it. Answers how many played.
pub async fn resume() -> usize {
    let list = std::mem::take(&mut *held());
    if list.is_empty() {
        return 0;
    }
    io::resume(list).await
}

/// Let go of what tug paused without playing it (the offer to play it again ran out).
pub async fn release() {
    let list = std::mem::take(&mut *held());
    if !list.is_empty() {
        io::release(list).await;
    }
}

#[cfg(windows)]
mod io {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use windows::core::Result;
    use windows::Foundation::TypedEventHandler;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status, PlaybackInfoChangedEventArgs,
    };

    use super::policy::{self, Playback, SessionView};
    use crate::ble::winrt::off_thread;

    /// A whole pass over the sessions (asking each app to pause or play is a round trip to it).
    const LIMIT: Duration = Duration::from_secs(10);

    /// A session tug paused: where it was left, and whether anyone played it since.
    pub struct Held {
        session: Session,
        view: SessionView,
        /// The PlaybackInfoChanged watch, if Windows allowed one.
        watch: Option<(i64, Arc<AtomicBool>)>,
    }

    impl Held {
        fn unwatch(&self) {
            if let Some((token, _)) = &self.watch {
                let _ = self.session.RemovePlaybackInfoChanged(*token);
            }
        }
    }

    fn playback(s: &Session) -> Result<Playback> {
        Ok(match s.GetPlaybackInfo()?.PlaybackStatus()? {
            Status::Playing => Playback::Playing,
            Status::Paused => Playback::Paused,
            _ => Playback::Other,
        })
    }

    fn view(s: &Session) -> Result<SessionView> {
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        Ok(SessionView {
            app_id: s.SourceAppUserModelId()?.to_string(),
            playback: playback(s)?,
            title: props.Title().map(|t| t.to_string()).unwrap_or_default(),
            artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(),
            // TimeSpan counts 100 ns ticks.
            position_ms: s
                .GetTimelineProperties()
                .and_then(|t| t.Position())
                .ok()
                .map(|p| p.Duration / 10_000),
        })
    }

    /// Notice the person playing it themselves (after the moment an app takes to settle).
    fn watch(s: &Session) -> Option<(i64, Arc<AtomicBool>)> {
        let paused_at = Instant::now();
        let played = Arc::new(AtomicBool::new(false));
        let flag = played.clone();
        let handler = TypedEventHandler::<Session, PlaybackInfoChangedEventArgs>::new(move |sender, _| {
            if let Some(s) = sender.as_ref() {
                let since = i64::try_from(paused_at.elapsed().as_millis()).unwrap_or(i64::MAX);
                if policy::played_by_person(playback(s)?, since) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            Ok(())
        });
        s.PlaybackInfoChanged(&handler).ok().map(|token| (token, played))
    }

    pub async fn pause(own_app_id: Option<String>) -> Vec<Held> {
        let done = off_thread(LIMIT, move || {
            let manager = Manager::RequestAsync()?.join()?;
            let mut held = Vec::new();
            for s in manager.GetSessions()? {
                let Ok(before) = view(&s) else { continue };
                if !policy::should_pause(&before, own_app_id.as_deref()) {
                    continue;
                }
                match s.TryPauseAsync().and_then(|op| op.join()) {
                    Ok(true) => {}
                    Ok(false) => {
                        log::info!("PC media: {} didn't take Pause", before.app_id);
                        continue;
                    }
                    Err(e) => {
                        log::info!("PC media: pausing {} failed: {}", before.app_id, e.message());
                        continue;
                    }
                }
                let watch = watch(&s);
                // Where it stopped, read after the pause; it's paused as far as tug is concerned.
                let after = view(&s).unwrap_or(before);
                held.push(Held {
                    session: s,
                    view: SessionView {
                        playback: Playback::Paused,
                        ..after
                    },
                    watch,
                });
            }
            Ok(held)
        })
        .await;
        match done {
            Ok(held) => {
                let apps: Vec<&str> = held.iter().map(|h| h.view.app_id.as_str()).collect();
                log::info!("PC media: paused {} for a ringing call {apps:?}", held.len());
                held
            }
            // Anything paused before the time ran out stays paused: tug can't tell what it was.
            Err(e) => {
                log::warn!("PC media: couldn't pause for a ringing call: {e}");
                Vec::new()
            }
        }
    }

    pub async fn resume(list: Vec<Held>) -> usize {
        let done = off_thread(LIMIT, move || {
            let mut played = 0;
            for h in &list {
                h.unwatch();
                let since = h.watch.as_ref().map(|(_, flag)| flag.load(Ordering::Relaxed));
                let now = view(&h.session).ok();
                if !policy::should_resume(&h.view, since, now.as_ref()) {
                    log::info!(
                        "PC media: leaving {} as it is (changed since tug paused it)",
                        h.view.app_id
                    );
                    continue;
                }
                match h.session.TryPlayAsync().and_then(|op| op.join()) {
                    Ok(true) => played += 1,
                    Ok(false) => log::info!("PC media: {} didn't take Play", h.view.app_id),
                    Err(e) => log::info!("PC media: playing {} failed: {}", h.view.app_id, e.message()),
                }
            }
            Ok(played)
        })
        .await;
        match done {
            Ok(n) => {
                log::info!("PC media: played {n} again after the call");
                n
            }
            Err(e) => {
                log::warn!("PC media: couldn't play again after the call: {e}");
                0
            }
        }
    }

    pub async fn release(list: Vec<Held>) {
        let done = off_thread(LIMIT, move || {
            for h in &list {
                h.unwatch();
            }
            Ok(list.len())
        })
        .await;
        match done {
            Ok(n) => log::info!("PC media: let go of {n} paused for a call"),
            Err(e) => log::warn!("PC media: couldn't let go of what was paused: {e}"),
        }
    }
}

#[cfg(not(windows))]
mod io {
    //! Not Windows: nothing to pause.
    pub struct Held;

    pub async fn pause(_own_app_id: Option<String>) -> Vec<Held> {
        Vec::new()
    }

    pub async fn resume(_list: Vec<Held>) -> usize {
        0
    }

    pub async fn release(_list: Vec<Held>) {}
}

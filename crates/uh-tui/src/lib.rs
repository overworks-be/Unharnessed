// The screen's arithmetic: colours and animations are computed in floats
// and land on 8-bit channels and 16-bit cell coordinates, each clamped to
// its range where it is computed; counts become floats only to scale them.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! uh-tui: the terminal UI. A live session over a running agent, as its
//! ACP client (`live`), and a recorded conversation replayed on the same
//! screen (`unharnessed --tui-replay`).

pub mod agent;
mod canvas;
mod cell;
mod chat;
mod clipboard;
mod connect;
mod desk;
mod draft;
mod files;
mod fold;
mod goal;
mod history;
mod kernel;
mod line;
pub mod live;
mod markdown;
mod mention;
mod message;
mod models;
mod motion;
mod past;
mod pick;
mod prefs;
mod providers;
mod recording;
pub mod replay;
mod settings;
mod slash;
mod text;
mod theme;
mod view;

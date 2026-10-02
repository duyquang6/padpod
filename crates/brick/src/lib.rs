// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! What every app in this repository shares on the TrimUI Brick Pro.
//!
//! - `canvas`: the 1024x768 panel as a double-buffered BGRA framebuffer.
//! - `text`: TTF text, spruce's theme font first and an embedded Nunito
//!   subset as the fallback.
//! - `input`: the pad as presses with repeat, plus scripted sessions for
//!   driving an app with no one at the buttons.
//! - `png`: screenshots.

pub mod canvas;
pub mod input;
pub mod png;
pub mod text;

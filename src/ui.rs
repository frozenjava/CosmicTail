// SPDX-License-Identifier: MIT

//! View code shared by the window and the applet.
//!
//! Both binaries render the same device list from the same `TailnetStatus`,
//! but they have different `Message` types, so everything here is generic over
//! the message and returns inert content. Interaction is the caller's job:
//! wrap these in whichever button the surrounding UI uses.

pub mod copy;
pub mod device_row;
pub mod format;
pub mod grouping;
pub mod panes;

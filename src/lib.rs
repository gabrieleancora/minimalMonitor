#![deny(unsafe_code)]

pub mod auth;
pub mod bounds;
pub mod config;
pub mod destination;
pub mod error;
pub mod monitoring;
pub mod notifications;
pub mod ownership;
pub mod probes;
pub mod scheduler;
pub mod storage;
pub mod web;
mod web_gui;
mod web_monitoring;
mod web_notifications;
mod web_security;

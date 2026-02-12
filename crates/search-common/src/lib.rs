//! Shared types, hashing, tokenization, and scoring for the Freenet search engine.
//!
//! All cross-node data uses CBOR serialization (ciborium) and integer arithmetic
//! with x10000 scaling (no floating-point). Provides bloom filters for state sync,
//! SHA-256 metadata hashing, Unicode normalization, and web container parsing.

pub mod bloom;
pub mod extraction;
pub mod hashing;
pub mod normalization;
pub mod scoring;
pub mod tokenization;
pub mod types;
pub mod web_container;

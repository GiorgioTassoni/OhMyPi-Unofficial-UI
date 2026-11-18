/**
 * The typed bridge to the Rust host.
 *
 * Re-exports all domain-specific bridge modules under `./bridge/`.
 */

export * from "./bridge/core";
export * from "./bridge/sessions";
export * from "./bridge/agents";
export * from "./bridge/terminals";
export * from "./bridge/settings";
export * from "./bridge/providers";
export * from "./bridge/skills";

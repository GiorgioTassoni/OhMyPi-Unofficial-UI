import { invoke } from "./core";

export interface ProviderAccountDto {
  id?: number | null;
  provider: string;
  credentialType: string;
  isCustom: boolean;
  baseUrl?: string | null;
  models: string[];
  createdAt?: number | null;
}

export interface CatalogProviderDto {
  id: string;
  name: string;
  authType: string;
  envVar?: string | null;
  description?: string | null;
}

export interface CustomProviderInput {
  id: string;
  baseUrl: string;
  apiKey?: string | null;
  api?: string | null;
  modelId: string;
  modelName?: string | null;
}

/** Get the catalog of supported OMP providers. */
export async function getCatalogProviders(): Promise<CatalogProviderDto[]> {
  return invoke<CatalogProviderDto[]>("get_catalog_providers");
}

/** Get currently configured provider credentials and custom endpoints. */
export async function getConfiguredProviders(): Promise<ProviderAccountDto[]> {
  return invoke<ProviderAccountDto[]>("get_configured_providers");
}
/** One public OpenRouter endpoint provider that can serve an OpenRouter model. */
export interface OpenRouterProvider {
  slug: string;
  name: string;
}

/** One model's saved provider allowlist, as the settings overview lists it. */
export interface OpenRouterRouting {
  modelId: string;
  providers: string[];
}

/** Read the per-model OpenRouter provider restriction; `null` means automatic routing. */
export async function getOpenRouterRouting(modelId: string): Promise<string[] | null> {
  return invoke<string[] | null>("get_openrouter_routing", { modelId });
}

/** Every model-level provider allowlist on disk, including empty ones. Never fetches. */
export async function listOpenRouterRoutings(): Promise<OpenRouterRouting[]> {
  return invoke<OpenRouterRouting[]>("list_openrouter_routings");
}

/** List the public endpoint providers for one OpenRouter model. */
export async function getOpenRouterProviders(modelId: string): Promise<OpenRouterProvider[]> {
  return invoke<OpenRouterProvider[]>("get_openrouter_providers", { modelId });
}

/** Save an allowlist, or remove this model's restriction with `null` for automatic routing. */
export async function setOpenRouterRouting(
  modelId: string,
  providers: string[] | null,
): Promise<void> {
  return invoke<void>("set_openrouter_routing", { modelId, providers });
}


/** Store an API key for a catalog provider in agent.db. */
export async function addProviderApiKey(provider: string, apiKey: string): Promise<void> {
  return invoke<void>("add_provider_api_key", { provider, apiKey });
}

/** Remove an authenticated credential from agent.db by SQLite ID. */
export async function removeProviderCredential(id: number): Promise<void> {
  return invoke<void>("remove_provider_credential", { id });
}

/** Add or update a custom provider in models.yml. */
export async function addCustomProvider(input: CustomProviderInput): Promise<void> {
  return invoke<void>("add_custom_provider", { input });
}

/** Remove a custom provider from models.yml. */
export async function removeCustomProvider(providerId: string): Promise<void> {
  return invoke<void>("remove_custom_provider", { providerId });
}

export interface ProviderTestResultDto {
  ok: boolean;
  latencyMs: number;
  modelCount: number;
  sampleModels: string[];
  message: string;
}

export interface OAuthWindowLimitDto {
  label: string;
  usedPercent: number;
  resetAt: number | null;
  resetAfterSeconds: number | null;
}

export interface OAuthUsageLimitDto {
  provider: string;
  displayName: string;
  planType: string | null;
  email: string | null;
  windows: OAuthWindowLimitDto[];
}

/** Fetch quota and rate limits for configured OAuth providers. */
export async function getOAuthUsageLimits(): Promise<OAuthUsageLimitDto[]> {
  return invoke<OAuthUsageLimitDto[]>("get_oauth_usage_limits");
}

/** Test connectivity and authentication for a provider via OMP runner. */
export async function testProviderConnection(provider: string): Promise<ProviderTestResultDto> {
  return invoke<ProviderTestResultDto>("test_provider_connection", { provider });
}


import { commands } from "../bindings";
import type { EndpointConfig } from "./state";
import type { PersistedSettings } from "./settings";

export type ModelRole = "agent" | "supervisor" | "fast";

export interface ModelCheck {
    role: ModelRole;
    config: EndpointConfig;
}

export type ModelAvailabilityStatus =
    | { role: ModelRole; config: EndpointConfig; available: true }
    | { role: ModelRole; config: EndpointConfig; available: false; error: string };

/**
 * Pure helper: given the persisted settings, return the list of endpoints
 * that should be probed on boot. Endpoints with an empty base URL or model
 * are skipped (they cannot be checked). The fast endpoint is skipped when
 * `fastEnabled` is false because it is not used by the executor.
 */
export function buildModelChecks(settings: PersistedSettings): ModelCheck[] {
    const candidates: [ModelRole, EndpointConfig, boolean][] = [
        ["agent", settings.agentConfig, true],
        ["supervisor", settings.supervisorConfig, true],
        ["fast", settings.fastConfig, settings.fastEnabled],
    ];
    return candidates
        .filter(([, config, enabled]) => enabled && !isBlank(config))
        .map(([role, config]) => ({ role, config }));
}

function isBlank(config: EndpointConfig): boolean {
    return config.baseUrl.trim() === "" || config.model.trim() === "";
}

export function formatModelStatus(status: ModelAvailabilityStatus): string {
    const { role, config } = status;
    if (status.available) {
        return `Model check: ${role} (${config.model} @ ${config.baseUrl}) is available`;
    }
    return `Model check: ${role} (${config.model} @ ${config.baseUrl}) is unavailable — ${status.error}`;
}

async function probe(check: ModelCheck): Promise<ModelAvailabilityStatus> {
    const { role, config } = check;
    const apiKey = config.apiKey === "" ? null : config.apiKey;
    const result = await commands.checkEndpoint(config.baseUrl, apiKey, config.model);
    if (result.status === "ok") {
        return { role, config, available: true };
    }
    return { role, config, available: false, error: result.error.message ?? "Unreachable" };
}

/**
 * Probe all configured endpoints in parallel. Failures surface as
 * `available: false` entries rather than throwing, so a single bad endpoint
 * does not mask the status of the others.
 */
export async function verifyConfiguredModels(
    settings: PersistedSettings,
): Promise<ModelAvailabilityStatus[]> {
    return Promise.all(buildModelChecks(settings).map(probe));
}

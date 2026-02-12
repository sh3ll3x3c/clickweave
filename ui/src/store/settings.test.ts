import { describe, it, expect } from "vitest";
import { toEndpoint } from "./settings";

describe("toEndpoint", () => {
  it("maps camelCase UI config to snake_case backend config", () => {
    const result = toEndpoint({
      baseUrl: "http://localhost:1234/v1",
      apiKey: "sk-test",
      model: "gpt-4",
    });
    expect(result).toEqual({
      base_url: "http://localhost:1234/v1",
      api_key: "sk-test",
      model: "gpt-4",
    });
  });

  it("converts empty apiKey to null", () => {
    const result = toEndpoint({
      baseUrl: "http://localhost:1234/v1",
      apiKey: "",
      model: "local",
    });
    expect(result.api_key).toBeNull();
  });
});

import { expect, test } from "vitest";
import { localAiConfig } from "./ai";

test("settings kept here for ECNU, no longer a preset, go on as a custom provider; from before the model saw pictures, it does not", () => {
  const ecnu = { provider: "ecnu", base_url: "https://chat.ecnu.edu.cn/open/api/v1", model: "ecnu-max", thinking: "off", api_key: "sk-1" };
  localStorage.setItem("ai:u1", JSON.stringify(ecnu));
  expect(localAiConfig("u1")).toEqual({ ...ecnu, provider: "custom", vision: false });
  const deepseek = { ...ecnu, provider: "deepseek", vision: true };
  localStorage.setItem("ai:u1", JSON.stringify(deepseek));
  expect(localAiConfig("u1")).toEqual(deepseek);
  localStorage.clear();
  expect(localAiConfig("u1")).toBeNull();
});

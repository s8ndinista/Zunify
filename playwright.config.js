import { defineConfig } from "@playwright/test";

const port = 4184;
const basePath = "/Zunify/";
const url = `http://127.0.0.1:${port}${basePath}`;
export default defineConfig({
  testDir: "./tests",
  workers: 1,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: url,
    viewport: { width: 1440, height: 900 },
    launchOptions: {
      executablePath: process.env.SPOTIFAST_TEST_CHROME,
      args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
    },
  },
  webServer: { command: "npm run preview", env: { PORT: String(port), BASE_PATH: basePath }, url },
});

import fs from "node:fs";
import path from "node:path";

const rootDir = process.cwd();
const packageJsonPath = path.join(rootDir, "package.json");
const tauriConfigPath = path.join(rootDir, "src-tauri", "tauri.conf.json");
const cargoTomlPath = path.join(rootDir, "src-tauri", "Cargo.toml");

const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
const version = packageJson.version;

if (typeof version !== "string" || version.trim() === "") {
  throw new Error("package.json version is missing or invalid");
}

const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
if (tauriConfig.version !== version) {
  tauriConfig.version = version;
  fs.writeFileSync(tauriConfigPath, `${JSON.stringify(tauriConfig, null, 2)}\n`);
}

const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
const versionLinePattern = /^version = ".*"$/m;

if (!versionLinePattern.test(cargoToml)) {
  throw new Error("Failed to locate version field in src-tauri/Cargo.toml");
}

const updatedCargoToml = cargoToml.replace(
  versionLinePattern,
  `version = "${version}"`
);

if (updatedCargoToml !== cargoToml) {
  fs.writeFileSync(cargoTomlPath, updatedCargoToml);
}

console.log(`Synchronized Tauri version to ${version}`);

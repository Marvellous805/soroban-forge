import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const sdkDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const rootDir = resolve(sdkDir, "../..");
const target = "wasm32v1-none";
const releaseDir = join(rootDir, "target", target, "release");
const generatedDir = join(rootDir, "target", "generated-typescript-clients");
const sdkClientsDir = join(sdkDir, "src", "clients");

const contracts = [
  {
    crate: "soroban-forge-escrow",
    wasm: "soroban_forge_escrow.wasm",
    module: "escrow",
  },
  {
    crate: "soroban-forge-vesting",
    wasm: "soroban_forge_vesting.wasm",
    module: "vesting",
    package: "vesting-client",
  },
  {
    crate: "soroban-forge-multi-sig-wallet",
    wasm: "soroban_forge_multi_sig_wallet.wasm",
    module: "multisig",
    package: "multi-sig-wallet-client",
  },
  {
    crate: "soroban-forge-marketplace-royalties",
    wasm: "soroban_forge_marketplace_royalties.wasm",
    module: "marketplace",
    package: "marketplace-royalties-client",
  },
  {
    crate: "soroban-forge-subscription-payments",
    wasm: "soroban_forge_subscription_payments.wasm",
    module: "subscription",
    package: "subscription-payments-client",
  },
  {
    crate: "soroban-forge-dao-governance",
    wasm: "soroban_forge_dao_governance.wasm",
    module: "dao",
    package: "dao-governance-client",
  },
];

function run(command, args, cwd = rootDir) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

console.log(`==> Building contract WASMs for target '${target}'`);
run("cargo", [
  "build",
  "--locked",
  "--release",
  "--target",
  target,
  ...contracts.flatMap(({ crate }) => ["--package", crate]),
]);

mkdirSync(sdkClientsDir, { recursive: true });

for (const contract of contracts) {
  const wasmPath = join(releaseDir, contract.wasm);
  if (!existsSync(wasmPath)) {
    throw new Error(`Expected WASM missing: ${wasmPath}`);
  }

  const outputDir = join(generatedDir, contract.module);
  rmSync(outputDir, { recursive: true, force: true });
  mkdirSync(outputDir, { recursive: true });
  console.log(`==> Generating ${contract.module} client from ${wasmPath}`);
  run("stellar", [
    "contract",
    "bindings",
    "typescript",
    "--wasm",
    wasmPath,
    "--output-dir",
    outputDir,
    "--overwrite",
  ]);

  const generatedSource = join(outputDir, "src", "index.ts");
  if (!existsSync(generatedSource)) {
    throw new Error(`Expected generated binding missing: ${generatedSource}`);
  }

  const clientClass = "export class Client extends ContractClient {";
  const generatedText = readFileSync(generatedSource, "utf8").replace(
    /[ \t]+(?=\r?$)/gm,
    "",
  );
  if (!generatedText.includes(clientClass)) {
    throw new Error(`Expected generated Client class in ${generatedSource}`);
  }
  writeFileSync(
    generatedSource,
    generatedText.replace(
      clientClass,
      `export interface Client {\n  readonly spec: ContractSpec;\n  txFromJSON<T = unknown>(json: string): AssembledTransaction<T>;\n}\n${clientClass}`,
    ),
  );
  cpSync(generatedSource, join(sdkClientsDir, `${contract.module}.ts`));

  if (contract.package) {
    const packageDir = join(rootDir, "packages", contract.package);
    rmSync(packageDir, { recursive: true, force: true });
    mkdirSync(packageDir, { recursive: true });
    cpSync(outputDir, packageDir, { recursive: true });
  }
}

console.log(`==> All six clients generated in ${sdkClientsDir} and standalone packages.`);
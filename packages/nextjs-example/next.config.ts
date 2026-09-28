/** @type {import('next').NextConfig} */
const path = require("path");

const nextConfig = {
  reactStrictMode: true,
  transpilePackages: ["@soroban-forge/escrow-client"],
  turbopack: {
    root: path.resolve(__dirname, ".."),
  },
}

export default nextConfig

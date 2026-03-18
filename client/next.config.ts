import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Container-friendly output for ECS deployment.
  output: "standalone",
  poweredByHeader: false,
  compress: true,
  experimental: {
    optimizePackageImports: ["react", "react-dom"],
  },
};

export default nextConfig;

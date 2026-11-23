/** @type {import('next').NextConfig} */
// With ORBIT_STATIC_DIR set, the dashboard is built as static files from exported JSON
// (GitHub Pages); otherwise it renders on each request from a running orbit-api.
const isStatic = Boolean(process.env.ORBIT_STATIC_DIR);

const nextConfig = {
  reactStrictMode: true,
  poweredByHeader: false,
  ...(isStatic && {
    output: "export",
    basePath: process.env.NEXT_BASE_PATH || "",
    trailingSlash: true,
    images: { unoptimized: true },
  }),
};

export default nextConfig;

#!/usr/bin/env node
import { writeFile } from "node:fs/promises";

const url = process.env.OPENAPI_URL ?? "http://localhost:8080/openapi.json";
const out = new URL("../openapi.json", import.meta.url);

const res = await fetch(url);
if (!res.ok) {
  console.error(`Failed to fetch ${url}: ${res.status}`);
  process.exit(1);
}
const spec = await res.json();
await writeFile(out, JSON.stringify(spec, null, 2) + "\n");
console.log(`Wrote ${out.pathname} from ${url}`);

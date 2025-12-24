import type { Idl } from "@coral-xyz/anchor";

// Kept in sync with /idl/teranium.json
// Bundled into the SDK for deterministic client-side instruction building.
import idlJson from "../../../idl/teranium.json";

export const TERANIUM_IDL = idlJson as unknown as Idl;

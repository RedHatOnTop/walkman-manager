/** Anodised body colours. Sony sold the A50 series and the ZX500 series in these. */
export type Finish = "graphite" | "black" | "silver" | "blue" | "red" | "green";

export interface FinishSpec {
  hi: string;
  mid: string;
  lo: string;
  side: string;
  /** Opacity of the chamfer's highlight. */
  edge: number;
}

export const FINISHES: Record<Finish, FinishSpec> = {
  graphite: { hi: "#8a8486", mid: "#46413f", lo: "#211e1e", side: "#635d5d", edge: 0.32 },
  black: { hi: "#727479", mid: "#323337", lo: "#151618", side: "#4c4e53", edge: 0.24 },
  silver: { hi: "#f1f2f4", mid: "#bfc3c9", lo: "#8a8f98", side: "#d6d9de", edge: 0.6 },
  blue: { hi: "#6a7fa6", mid: "#33456a", lo: "#18213a", side: "#4f6489", edge: 0.26 },
  red: { hi: "#a85563", mid: "#652a35", lo: "#34131a", side: "#86404c", edge: 0.26 },
  green: { hi: "#9cada6", mid: "#5d6e68", lo: "#323d39", side: "#7e8f88", edge: 0.3 },
};

export const FINISH_ORDER: Finish[] = ["graphite", "black", "silver", "blue", "red", "green"];

import bjose2345 from "../assets/developers/bjose2345.png";
import unMagoSinNombre from "../assets/developers/un-mago-sin-nombre.jpg";
import type { Developer } from "./systemReport";

export const developers: readonly Developer[] = [
  {
    id: "bjose2345",
    name: "bjose2345",
    quote: "Ando farmeando aura",
    quoteEmoji: "😎",
    effect: "aura",
    portrait: bjose2345,
  },
  {
    id: "un-mago-sin-nombre",
    name: "Un Mago Sin Nombre",
    quote: "Conjurando… y a veces compila",
    effect: "arcane",
    portrait: unMagoSinNombre,
  },
];

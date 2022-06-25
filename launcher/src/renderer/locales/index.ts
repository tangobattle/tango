import * as en from "./en";
import * as es from "./es";
import * as ja from "./ja";
import * as pt_BR from "./pt-BR";
import * as zh_Hans from "./zh-Hans";

export default { en, ja, "zh-Hans": zh_Hans, "pt-BR": pt_BR, es } as {
  [language: string]: {
    default: { [namespace: string]: { [key: string]: string } };
  };
};

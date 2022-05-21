import * as en from "./en";
import * as ja from "./ja";
import * as zh_Hans from "./zh-Hans";
import * as es from "./es";

export default { en, ja, "zh-Hans": zh_Hans, es } as {
  [language: string]: {
    default: { [namespace: string]: { [key: string]: string } };
  };
};

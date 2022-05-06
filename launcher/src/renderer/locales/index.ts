import * as en from "./en";
import * as ja from "./ja";
import * as zh_Hans from "./zh-Hans";

export default { en, ja, "zh-Hans": zh_Hans } as {
  [language: string]: {
    default: { [namespace: string]: { [key: string]: string } };
  };
};

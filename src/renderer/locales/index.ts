import * as en from "./en";
import * as ja from "./ja";

export default { en, ja } as {
  [language: string]: {
    default: { [namespace: string]: { [key: string]: string } };
  };
};

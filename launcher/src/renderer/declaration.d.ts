// Required to allow importing images with webpack in typescript
declare module "*.png";
declare module "*.jpg";
declare module "*.svg";

declare namespace Intl {
  class ListFormat {
    constructor(locales?: string | string[], options?: any);
    public format(items: string[]): string;
  }
}

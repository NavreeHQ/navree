export const COLOUR_TOKENS: string[];
export const CSS_TOKENS: string[];
export const SEMANTIC: [name: string, value: string][];

export interface Theme {
  id: string;
  label: string;
  description: string;
  light: Record<string, string>;
  dark: Record<string, string>;
}

export function readThemes(source: { tokensCss: string; themeRs: string }): Theme[];
export function themesCss(themes: Theme[]): string;
export function themesTs(themes: Theme[]): string;

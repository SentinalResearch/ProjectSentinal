/**
 * Recipe templates use `{name}` for parameters and `{{` / `}}` for literal
 * braces, the grammar `protocol::Recipe` validates and renders.
 */

/** Escape a literal command fragment so its braces stay literal. */
export function escapeTemplateLiteral(text: string): string {
  return text.replace(/\{/g, "{{").replace(/\}/g, "}}");
}

/** Display-only rendering: the daemon renders and shell-quotes for real. */
export function previewTemplate(template: string, values: Record<string, string>): string {
  return template.replace(/\{\{|\}\}|\{(\w+)\}/g, (match, key: string | undefined) => {
    if (key === undefined) return match[0];
    return values[key] ?? match;
  });
}

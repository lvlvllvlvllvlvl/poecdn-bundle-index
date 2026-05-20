use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use anyhow::Result;

pub struct FileItem {
    pub name: String,
    pub hash: String,
    pub bundle: String,
    pub offset: String,
    pub size: String,
}

#[derive(Default)]
pub struct Node {
    pub files: Vec<FileItem>,
    pub children: BTreeMap<String, Node>,
}

impl Node {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_file(&mut self, path: &str, item: FileItem) {
        let mut current = self;
        if !path.is_empty() {
            for part in path.split('/') {
                if part.is_empty() { continue; }
                current = current.children.entry(part.to_string()).or_default();
            }
        }
        current.files.push(item);
    }

    fn count_nested_dirs(&self) -> usize {
        let mut count = self.children.len();
        for child in self.children.values() {
            count += child.count_nested_dirs();
        }
        count
    }

    fn get_all_nested_dirs(&self, prefix: &str) -> Vec<String> {
        let mut result = Vec::new();
        for (name, child) in &self.children {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            result.push(path.clone());
            result.extend(child.get_all_nested_dirs(&path));
        }
        result
    }

    fn render_tree_recursive(&self) -> String {
        self.render_tree_recursive_internal("")
    }

    fn render_tree_recursive_internal(&self, current_rel_path: &str) -> String {
        if self.children.is_empty() {
            return String::new();
        }
        let mut html = String::from("<ul>\n");
        for (name, child) in &self.children {
            let next_rel_path = if current_rel_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", current_rel_path, name)
            };
            html.push_str(&format!(
                "<li><a href=\"{}/index.html\">{}</a>",
                next_rel_path, name
            ));
            html.push_str(&child.render_tree_recursive_internal(&next_rel_path));
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
        html
    }

    pub fn export(&self, current_path: &Path, relative_path_str: &str) -> Result<()> {
        fs::create_dir_all(current_path)?;

        // 1. dirs.txt
        let mut dirs_txt = fs::File::create(current_path.join("dirs.txt"))?;
        let nested_dirs = self.get_all_nested_dirs("");
        for dir in &nested_dirs {
            writeln!(dirs_txt, "{}", dir)?;
        }

        // 2. files.csv
        let mut files_csv = csv::Writer::from_path(current_path.join("files.csv"))?;
        files_csv.write_record(&["hash", "file", "bundle", "offset", "size"])?;
        for file in &self.files {
            files_csv.write_record(&[
                &file.hash,
                &file.name, // using name as 'file'
                &file.bundle,
                &file.offset,
                &file.size,
            ])?;
        }
        files_csv.flush()?;

        // 3. index.html
        let mut html = fs::File::create(current_path.join("index.html"))?;
        let title = if relative_path_str.is_empty() {
            "Root".to_string()
        } else {
            relative_path_str.to_string()
        };

        writeln!(html, "<!DOCTYPE html>")?;
        writeln!(html, "<html>")?;
        writeln!(html, "<head><title>Index of {}</title></head>", title)?;
        writeln!(html, "<body>")?;
        writeln!(html, "<h1>Index of {}</h1>", title)?;
        
        if !relative_path_str.is_empty() {
            writeln!(html, "<p><a href=\"../index.html\">Up one level</a></p>")?;
        }

        writeln!(html, "<h2>Directories</h2>")?;
        let nested_count = self.count_nested_dirs();
        if nested_count > 30 {
            writeln!(html, "<ul>")?;
            for name in self.children.keys() {
                writeln!(html, "<li><a href=\"{}/index.html\">{}</a></li>", name, name)?;
            }
            writeln!(html, "</ul>")?;
        } else {
            writeln!(html, "{}", self.render_tree_recursive())?;
        }

        writeln!(html, "<h2>Files</h2>")?;
        writeln!(html, "<table border=\"1\">")?;
        writeln!(html, "<thead><tr><th>Name</th><th>Hash</th><th>Bundle</th><th>Offset</th><th>Size</th></tr></thead>")?;
        writeln!(html, "<tbody>")?;
        for file in &self.files {
            writeln!(html, "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                file.name, file.hash, file.bundle, file.offset, file.size)?;
        }
        writeln!(html, "</tbody></table>")?;
        writeln!(html, "</body></html>")?;

        // Recursive export
        for (name, child) in &self.children {
            let next_path = current_path.join(name);
            let next_relative = if relative_path_str.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", relative_path_str, name)
            };
            child.export(&next_path, &next_relative)?;
        }

        Ok(())
    }
}

pub fn generate_root_index(output_dir: &Path) -> Result<()> {
    let mut html = fs::File::create(output_dir.join("index.html"))?;

    writeln!(html, "<!DOCTYPE html>")?;
    writeln!(html, "<html>")?;
    writeln!(html, "<head>")?;
    writeln!(html, "  <meta charset=\"UTF-8\">")?;
    writeln!(html, "  <title>Path of Exile CDN Bundle Index</title>")?;
    writeln!(html, "  <style>")?;
    writeln!(html, "    body {{ font-family: sans-serif; line-height: 1.6; max-width: 800px; margin: 2rem auto; padding: 0 1rem; }}")?;
    writeln!(html, "    h1 {{ border-bottom: 2px solid #eee; padding-bottom: 0.5rem; }}")?;
    writeln!(html, "    h2 {{ margin-top: 2rem; color: #333; }}")?;
    writeln!(html, "    ul {{ list-style-type: none; padding-left: 0; }}")?;
    writeln!(html, "    li {{ margin-bottom: 0.5rem; }}")?;
    writeln!(html, "    .patch-block {{ background: #f9f9f9; padding: 1rem; border-radius: 4px; border: 1px solid #eee; }}")?;
    writeln!(html, "    .links {{ display: flex; gap: 1rem; flex-wrap: wrap; margin-top: 0.5rem; }}")?;
    writeln!(html, "    .links a {{ text-decoration: none; background: #007bff; color: white; padding: 0.2rem 0.6rem; border-radius: 3px; font-size: 0.9rem; }}")?;
    writeln!(html, "    .links a:hover {{ background: #0056b3; }}")?;
    writeln!(html, "    .db-links {{ font-size: 0.85rem; color: #666; margin-top: 1rem; border-top: 1px solid #eee; pt: 0.5rem; }}")?;
    writeln!(html, "    .db-links a {{ margin-right: 0.5rem; color: #0066cc; }}")?;
    writeln!(html, "  </style>")?;
    writeln!(html, "</head>")?;
    writeln!(html, "<body>")?;
    writeln!(html, "<h1>Path of Exile CDN Bundle Index</h1>")?;

    for game in &["poe1", "poe2"] {
        let game_dir = output_dir.join(game);
        if game_dir.exists() {
            writeln!(html, "<h2>{}</h2>", game.to_uppercase())?;
            
            // Find patch directory
            let patch_dir_root = game_dir.join("patch.poecdn.com");
            if patch_dir_root.exists() {
                if let Ok(entries) = fs::read_dir(&patch_dir_root) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            let patch_name = entry.file_name().to_string_lossy().into_owned();
                            let rel_patch_path = format!("{}/patch.poecdn.com/{}", game, patch_name);
                            
                            writeln!(html, "<div class=\"patch-block\">")?;
                            writeln!(html, "<strong>Patch: {}</strong>", patch_name)?;
                            writeln!(html, "<div class=\"links\">")?;
                            writeln!(html, "  <a href=\"{}/files/index.html\">Browse Files</a>", rel_patch_path)?;
                            writeln!(html, "  <a href=\"{}/bundles/index.html\">Browse Bundles</a>", rel_patch_path)?;
                            writeln!(html, "</div>")?;
                            
                            writeln!(html, "<div class=\"db-links\">")?;
                            writeln!(html, "  Downloads: ")?;
                            writeln!(html, "  <a href=\"{}/bundle_index.sqlite\">SQLite DB</a>", game)?;
                            writeln!(html, "  <a href=\"{}/bundles.sql\">bundles.sql</a>", game)?;
                            writeln!(html, "  <a href=\"{}/dirs.sql\">dirs.sql</a>", game)?;
                            writeln!(html, "  <a href=\"{}/files.sql\">files.sql</a>", game)?;
                            writeln!(html, "  <a href=\"{}/version.sql\">version.sql</a>", game)?;
                            writeln!(html, "</div>")?;
                            writeln!(html, "</div>")?;
                        }
                    }
                }
            } else {
                 writeln!(html, "<p>No data found for {}.</p>", game)?;
            }
        }
    }

    writeln!(html, "</body></html>")?;
    Ok(())
}

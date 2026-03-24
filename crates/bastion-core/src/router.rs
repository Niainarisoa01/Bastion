use hyper::Method;
use smallvec::SmallVec;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum ParamType {
    None,
    /// :param — capture un segment
    Named(Arc<str>),
    /// *wildcard — capture tout le reste
    CatchAll(Arc<str>),
}

#[derive(Clone, Debug)]
pub struct Route<T> {
    pub value: T,
    pub methods: Vec<Method>,
    pub strip_prefix: Option<String>,
    pub add_prefix: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TrieNode<T> {
    pub prefix: String,
    pub routes: Vec<Route<T>>,
    pub children: Vec<TrieNode<T>>,
    pub param_type: ParamType,
}

impl<T> Default for TrieNode<T> {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            routes: Vec::new(),
            children: Vec::new(),
            param_type: ParamType::None,
        }
    }
}

pub struct RouteMatch<'a, T> {
    pub value: &'a T,
    pub params: SmallVec<[(&'a str, &'a str); 4]>,
    pub rewritten_path: Option<String>,
}

#[derive(Clone)]
pub struct RadixTrie<T> {
    root: TrieNode<T>,
}

impl<T> Default for RadixTrie<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn common_prefix_len(s1: &str, s2: &str) -> usize {
    s1.chars().zip(s2.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum()
}

impl<T> RadixTrie<T> {
    pub fn new() -> Self {
        Self {
            root: TrieNode::default(),
        }
    }

    pub fn insert(
        &mut self,
        path: &str,
        methods: Vec<Method>,
        value: T,
        strip_prefix: Option<String>,
        add_prefix: Option<String>,
    ) {
        let route = Route {
            value,
            methods,
            strip_prefix,
            add_prefix,
        };
        
        let segments = parse_path(path);
        let mut current = &mut self.root;
        
        for segment in segments {
            match segment.param_type {
                ParamType::None => {
                    let mut text = segment.prefix;
                    while !text.is_empty() {
                        let mut match_idx = None;
                        for (i, child) in current.children.iter().enumerate() {
                            if child.param_type == ParamType::None && child.prefix.starts_with(text.chars().next().unwrap()) {
                                match_idx = Some(i);
                                break;
                            }
                        }
                        
                        if let Some(i) = match_idx {
                            let lcp = common_prefix_len(&current.children[i].prefix, text);
                            if lcp < current.children[i].prefix.len() {
                                // Split needed
                                let split_child = TrieNode {
                                    prefix: current.children[i].prefix[lcp..].to_string(),
                                    routes: std::mem::take(&mut current.children[i].routes),
                                    children: std::mem::take(&mut current.children[i].children),
                                    param_type: ParamType::None,
                                };
                                current.children[i].prefix = text[..lcp].to_string();
                                current.children[i].children.push(split_child);
                            }
                            
                            text = &text[lcp..];
                            current = &mut current.children[i];
                        } else {
                            // No child starts with the same character, create one
                            let new_node = TrieNode {
                                prefix: text.to_string(),
                                routes: Vec::new(),
                                children: Vec::new(),
                                param_type: ParamType::None,
                            };
                            current.children.push(new_node);
                            text = "";
                            current = current.children.last_mut().unwrap();
                        }
                    }
                }
                ptype => {
                    let mut match_idx = None;
                    for (i, child) in current.children.iter().enumerate() {
                        if child.param_type == ptype {
                            match_idx = Some(i);
                            break;
                        }
                    }
                    if let Some(i) = match_idx {
                        current = &mut current.children[i];
                    } else {
                        let new_node = TrieNode {
                            prefix: String::new(),
                            routes: Vec::new(),
                            children: Vec::new(),
                            param_type: ptype,
                        };
                        current.children.push(new_node);
                        current = current.children.last_mut().unwrap();
                    }
                }
            }
        }
        
        current.routes.push(route);
    }

    pub fn lookup<'a>(&'a self, method: &Method, path: &'a str) -> Option<RouteMatch<'a, T>> {
        let mut params = SmallVec::new();
        self.match_node(&self.root, path, method, &mut params).map(|route| {
            let mut rewritten_path = None;
            if route.strip_prefix.is_some() || route.add_prefix.is_some() {
                let mut p = path.to_string();
                if let Some(ref strip) = route.strip_prefix {
                    if p.starts_with(strip) {
                        p = p[strip.len()..].to_string();
                        if !p.starts_with('/') {
                            p = format!("/{}", p);
                        }
                    }
                }
                if let Some(ref add) = route.add_prefix {
                    let mut p_trim = p.as_str();
                    if p_trim.starts_with('/') && add.ends_with('/') {
                        p_trim = &p_trim[1..];
                    }
                    if !p_trim.starts_with('/') && !add.ends_with('/') {
                        p = format!("{}/{}", add, p_trim);
                    } else {
                        p = format!("{}{}", add, p_trim);
                    }
                }
                rewritten_path = Some(p);
            }

            RouteMatch {
                value: &route.value,
                params,
                rewritten_path,
            }
        })
    }

    fn match_node<'a, 'b>(
        &'a self,
        node: &'a TrieNode<T>,
        path: &'b str,
        method: &Method,
        params: &mut SmallVec<[(&'a str, &'b str); 4]>,
    ) -> Option<&'a Route<T>> {
        let remaining_path = match node.param_type {
            ParamType::None => {
                if !path.starts_with(&node.prefix) {
                    return None;
                }
                &path[node.prefix.len()..]
            }
            ParamType::Named(ref name) => {
                let end_idx = path.find('/').unwrap_or(path.len());
                let val = &path[..end_idx];
                if val.is_empty() {
                    return None;
                }
                params.push((name.as_ref(), val));
                &path[end_idx..]
            }
            ParamType::CatchAll(ref name) => {
                let val = path;
                params.push((name.as_ref(), val));
                ""
            }
        };

        if remaining_path.is_empty() {
            for route in &node.routes {
                if route.methods.is_empty() || route.methods.contains(method) {
                    return Some(route);
                }
            }
        }

        // 1. Static children
        for child in &node.children {
            if child.param_type == ParamType::None {
                if let Some(r) = self.match_node(child, remaining_path, method, params) {
                    return Some(r);
                }
            }
        }

        // 2. Named children
        for child in &node.children {
            if matches!(child.param_type, ParamType::Named(_)) {
                let initial_len = params.len();
                if let Some(r) = self.match_node(child, remaining_path, method, params) {
                    return Some(r);
                }
                params.truncate(initial_len);
            }
        }

        // 3. Catch-all children
        for child in &node.children {
            if matches!(child.param_type, ParamType::CatchAll(_)) {
                let initial_len = params.len();
                if let Some(r) = self.match_node(child, remaining_path, method, params) {
                    return Some(r);
                }
                params.truncate(initial_len);
            }
        }

        None
    }
}

// Interne : structure temporaire pour parser le chemin d'insertion
struct PathSegment<'a> {
    prefix: &'a str,
    param_type: ParamType,
}

fn parse_path(path: &str) -> Vec<PathSegment<'_>> {
    let mut segments = Vec::new();
    let mut current_idx = 0;
    
    while current_idx < path.len() {
        let remaining = &path[current_idx..];
        
        // Find next param start
        let mut next_param_start = remaining.len();
        if let Some(colon_idx) = remaining.find(':') {
            next_param_start = next_param_start.min(colon_idx);
        }
        if let Some(star_idx) = remaining.find('*') {
            next_param_start = next_param_start.min(star_idx);
        }
        
        // Push static segment if any
        if next_param_start > 0 {
            segments.push(PathSegment {
                prefix: &path[current_idx..current_idx + next_param_start],
                param_type: ParamType::None,
            });
            current_idx += next_param_start;
        }
        
        if current_idx < path.len() {
            let remaining = &path[current_idx..];
            if remaining.starts_with(':') {
                // Find next slash
                let end_offset = remaining.find('/').unwrap_or(remaining.len());
                let param_name = &remaining[1..end_offset];
                segments.push(PathSegment {
                    prefix: "",
                    param_type: ParamType::Named(Arc::from(param_name)),
                });
                current_idx += end_offset;
            } else if remaining.starts_with('*') {
                let param_name = &remaining[1..];
                segments.push(PathSegment {
                    prefix: "",
                    param_type: ParamType::CatchAll(Arc::from(param_name)),
                });
                break;
            }
        }
    }
    
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_compression() {
        let mut trie = RadixTrie::new();
        trie.insert("/api", vec![], "api", None, None);
        trie.insert("/app", vec![], "app", None, None);
        
        let m1 = trie.lookup(&Method::GET, "/api").unwrap();
        assert_eq!(*m1.value, "api");
        
        let m2 = trie.lookup(&Method::GET, "/app").unwrap();
        assert_eq!(*m2.value, "app");

        // The root should have one child "/ap" with 2 children "i" and "p"
        let root_children = &trie.root.children;
        assert_eq!(root_children.len(), 1);
        assert_eq!(root_children[0].prefix, "/ap");
        assert_eq!(root_children[0].children.len(), 2);
    }

    #[test]
    fn test_exact_match() {
        let mut trie = RadixTrie::new();
        trie.insert("/api/users", web_methods(vec![Method::GET]), "users_get", None, None);
        trie.insert("/api/users", web_methods(vec![Method::POST]), "users_post", None, None);
        
        let match1 = trie.lookup(&Method::GET, "/api/users").unwrap();
        assert_eq!(*match1.value, "users_get");
        
        let match2 = trie.lookup(&Method::POST, "/api/users").unwrap();
        assert_eq!(*match2.value, "users_post");
    }

    #[test]
    fn test_no_match_returns_none() {
        let mut trie = RadixTrie::new();
        trie.insert("/api/users", web_methods(vec![Method::GET]), "users_get", None, None);
        
        assert!(trie.lookup(&Method::PUT, "/api/users").is_none());
        assert!(trie.lookup(&Method::GET, "/api/uses").is_none());
    }

    #[test]
    fn test_prefix_match() {
        let mut trie = RadixTrie::new();
        trie.insert("/api/", vec![], "api_base", None, None);
        trie.insert("/api/v1/", vec![], "v1_base", None, None);
        
        assert_eq!(*trie.lookup(&Method::GET, "/api/").unwrap().value, "api_base");
        assert_eq!(*trie.lookup(&Method::GET, "/api/v1/").unwrap().value, "v1_base");
        assert!(trie.lookup(&Method::GET, "/api/v1/users").is_none());
    }

    #[test]
    fn test_param_extraction() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id", vec![], "user_id", None, None);
        trie.insert("/users/:id/posts/:post_id", vec![], "user_post", None, None);
        
        let m1 = trie.lookup(&Method::GET, "/users/123").unwrap();
        assert_eq!(*m1.value, "user_id");
        assert_eq!(m1.params.len(), 1);
        assert_eq!(m1.params[0], ("id", "123"));
        
        let m2 = trie.lookup(&Method::GET, "/users/123/posts/456").unwrap();
        assert_eq!(*m2.value, "user_post");
        assert_eq!(m2.params.len(), 2);
        assert_eq!(m2.params[0], ("id", "123"));
        assert_eq!(m2.params[1], ("post_id", "456"));
    }

    #[test]
    fn test_wildcard() {
        let mut trie = RadixTrie::new();
        trie.insert("/static/*filepath", vec![], "static", None, None);
        
        let m = trie.lookup(&Method::GET, "/static/css/main.css").unwrap();
        assert_eq!(*m.value, "static");
        assert_eq!(m.params.len(), 1);
        assert_eq!(m.params[0], ("filepath", "css/main.css"));
    }

    #[test]
    fn test_priority_exact_over_param() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/me", vec![], "me", None, None);
        trie.insert("/users/:id", vec![], "id", None, None);
        trie.insert("/*all", vec![], "catchall", None, None);
        
        assert_eq!(*trie.lookup(&Method::GET, "/users/me").unwrap().value, "me");
        assert_eq!(*trie.lookup(&Method::GET, "/users/123").unwrap().value, "id");
        assert_eq!(*trie.lookup(&Method::GET, "/about").unwrap().value, "catchall");
    }

    #[test]
    fn test_strip_prefix() {
        let mut trie = RadixTrie::new();
        trie.insert("/api/v1/users", vec![], "v1_users", Some("/api/v1".to_string()), Some("/v2".to_string()));
        
        let m = trie.lookup(&Method::GET, "/api/v1/users").unwrap();
        assert_eq!(*m.value, "v1_users");
        assert_eq!(m.rewritten_path.unwrap(), "/v2/users");
    }

    #[test]
    fn test_method_filtering() {
        let mut trie = RadixTrie::new();
        trie.insert("/submit", web_methods(vec![Method::POST]), "submit_post", None, None);
        trie.insert("/submit", web_methods(vec![Method::PUT]), "submit_put", None, None);

        assert_eq!(*trie.lookup(&Method::POST, "/submit").unwrap().value, "submit_post");
        assert_eq!(*trie.lookup(&Method::PUT, "/submit").unwrap().value, "submit_put");
        assert!(trie.lookup(&Method::GET, "/submit").is_none());
    }

    #[test]
    fn test_empty_methods_allows_all() {
        let mut trie = RadixTrie::new();
        trie.insert("/public", vec![], "public_all", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/public").unwrap().value, "public_all");
        assert_eq!(*trie.lookup(&Method::OPTIONS, "/public").unwrap().value, "public_all");
    }

    #[test]
    fn test_multiple_params_same_prefix() {
        let mut trie = RadixTrie::new();
        trie.insert("/posts/:id/comments", vec![], "comments", None, None);
        trie.insert("/posts/:id/likes", vec![], "likes", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/posts/1/comments").unwrap().value, "comments");
        assert_eq!(*trie.lookup(&Method::GET, "/posts/1/likes").unwrap().value, "likes");
    }

    #[test]
    fn test_deep_compression() {
        let mut trie = RadixTrie::new();
        trie.insert("/a/b/c/d/e", vec![], "abcde", None, None);
        trie.insert("/a/b/c/d/f", vec![], "abcdf", None, None);
        trie.insert("/a/b/x/y/z", vec![], "abxyz", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/a/b/c/d/e").unwrap().value, "abcde");
        assert_eq!(*trie.lookup(&Method::GET, "/a/b/c/d/f").unwrap().value, "abcdf");
        assert_eq!(*trie.lookup(&Method::GET, "/a/b/x/y/z").unwrap().value, "abxyz");
    }

    #[test]
    fn test_param_with_slash() {
        let mut trie = RadixTrie::new();
        trie.insert("/items/:id", vec![], "item", None, None);
        
        assert!(trie.lookup(&Method::GET, "/items/123/456").is_none());
        assert_eq!(*trie.lookup(&Method::GET, "/items/123").unwrap().value, "item");
    }

    #[test]
    fn test_catchall_priority() {
        let mut trie = RadixTrie::new();
        trie.insert("/files/*all", vec![], "catchall", None, None);
        trie.insert("/files/specific", vec![], "specific", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/files/specific").unwrap().value, "specific");
        assert_eq!(*trie.lookup(&Method::GET, "/files/other/path").unwrap().value, "catchall");
    }

    #[test]
    fn test_overlapping_params() {
        let mut trie = RadixTrie::new();
        trie.insert("/:resource/list", vec![], "resource_list", None, None);
        trie.insert("/users/:action", vec![], "users_action", None, None);

        let m1 = trie.lookup(&Method::GET, "/users/list").unwrap();
        assert_eq!(*m1.value, "users_action");
    }

    #[test]
    fn test_strip_prefix_without_add() {
        let mut trie = RadixTrie::new();
        trie.insert("/v1/api/data", vec![], "data", Some("/v1/api".to_string()), None);
        
        let m = trie.lookup(&Method::GET, "/v1/api/data").unwrap();
        assert_eq!(m.rewritten_path.unwrap(), "/data");
    }

    #[test]
    fn test_add_prefix_without_strip() {
        let mut trie = RadixTrie::new();
        trie.insert("/data", vec![], "data", None, Some("/v2".to_string()));
        
        let m = trie.lookup(&Method::GET, "/data").unwrap();
        assert_eq!(m.rewritten_path.unwrap(), "/v2/data");
    }

    #[test]
    fn test_rewrite_complex_path() {
        let mut trie = RadixTrie::new();
        trie.insert("/old/path/resource", vec![], "res", Some("/old/path".to_string()), Some("/new".to_string()));
        
        let m = trie.lookup(&Method::GET, "/old/path/resource").unwrap();
        assert_eq!(m.rewritten_path.unwrap(), "/new/resource");
    }

    #[test]
    fn test_multiple_methods_same_path() {
        let mut trie = RadixTrie::new();
        trie.insert("/auth", web_methods(vec![Method::OPTIONS, Method::POST]), "auth", None, None);

        assert_eq!(*trie.lookup(&Method::OPTIONS, "/auth").unwrap().value, "auth");
        assert_eq!(*trie.lookup(&Method::POST, "/auth").unwrap().value, "auth");
        assert!(trie.lookup(&Method::GET, "/auth").is_none());
    }

    #[test]
    fn test_root_path() {
        let mut trie = RadixTrie::new();
        trie.insert("/", vec![], "root", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/").unwrap().value, "root");
        assert!(trie.lookup(&Method::GET, "/other").is_none());
    }

    #[test]
    fn test_unicode_paths() {
        let mut trie = RadixTrie::new();
        trie.insert("/café", vec![], "cafe", None, None);
        trie.insert("/caféteria", vec![], "cafeteria", None, None);

        assert_eq!(*trie.lookup(&Method::GET, "/café").unwrap().value, "cafe");
        assert_eq!(*trie.lookup(&Method::GET, "/caféteria").unwrap().value, "cafeteria");
    }
    
    #[test]
    fn test_very_long_static_path() {
        let mut trie = RadixTrie::new();
        trie.insert("/a/very/long/static/path/that/goes/on/and/on", vec![], "long", None, None);
        
        assert_eq!(*trie.lookup(&Method::GET, "/a/very/long/static/path/that/goes/on/and/on").unwrap().value, "long");
        assert!(trie.lookup(&Method::GET, "/a/very/long/static/path").is_none());
    }

    fn web_methods(m: Vec<Method>) -> Vec<Method> {
        m
    }
}

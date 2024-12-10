pub fn get_urls(str:&String) -> Vec<String> {
  str.split(",").map(|s| s.to_string()).collect()
}
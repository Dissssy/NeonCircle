// use std::collections::HashMap;

// use serde::{Deserialize, Serialize};

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub struct ShitGPT {
//     table: HashMap<String, HashMap<String, usize>>,
//     bufsize: usize,
// }

// impl ShitGPT {
//     pub fn new(bufsize: usize) -> ShitGPT {
//         ShitGPT {
//             table: HashMap::new(),
//             bufsize,
//         }
//     }
//     pub fn train(&mut self, text: String) {
//         let text = text.trim().to_lowercase();
//         if text.is_empty() {
//             return;
//         }
//         for string in text.replace('\r', "\n").split('\n') {
//             let string = string.trim();
//             if string.is_empty() {
//                 continue;
//             } else {
//                 let string = format!("{}\n", string);
//                 let mut buf = SizedVec::new(self.bufsize);
//                 for char in string.chars() {
//                     // table will be mapped from current buffer to possible next chars, and their frequencies (cumulative)
//                     let table = self.table.entry(buf.get_string()).or_insert(HashMap::new());
//                     let freq = table.entry(char.to_string()).or_insert(0);
//                     *freq += 1;
//                     buf.push(char);
//                 }
//             }
//         }
//     }
//     pub fn generate(&self) -> String {
//         // we will take a starting point of \n and use the table to determine by random choice based on the weights, which char to add next, and then repeat until we hit a \n
//         let mut buf = SizedVec::new(self.bufsize);
//         let mut string = String::new();
//         loop {
//             let blankmap = HashMap::new();
//             let table = self.table.get(&buf.get_string()).unwrap_or(&blankmap);
//             let mut total = 0;
//             for freq in table.values() {
//                 total += freq;
//             }
//             if total != 0 {
//                 let mut choice = rand::random::<usize>() % total;
//                 for (char, freq) in table {
//                     if choice < *freq {
//                         string.push_str(char);
//                         buf.push(char.chars().next().unwrap());
//                         break;
//                     } else {
//                         choice -= freq;
//                     }
//                 }
//             } else {
//                 buf.push('\n')
//             }
//             if string.len() > 500 {
//                 buf.push('\n')
//             }
//             if buf.get_string().ends_with('\n') {
//                 break;
//             }
//         }
//         string.trim().to_string()
//     }
//     pub fn generate_without_weights(&self) -> String {
//         // we will take a starting point of \n and use the table to determine by random choice based on the weights, which char to add next, and then repeat until we hit a \n
//         let mut buf = SizedVec::new(self.bufsize);
//         let mut string = String::new();
//         loop {
//             let blankmap = HashMap::new();
//             let table = self.table.get(&buf.get_string()).unwrap_or(&blankmap);
//             // just select a random char from the next chars
//             if table.is_empty() {
//                 buf.push('\n')
//             } else {
//                 let choice = rand::random::<usize>() % table.len();
//                 string.push_str(table.keys().nth(choice).unwrap());
//                 buf.push(table.keys().nth(choice).unwrap().chars().next().unwrap());
//             }
//             if string.len() > 500 {
//                 buf.push('\n')
//             }
//             if buf.get_string().ends_with('\n') {
//                 break;
//             }
//         }
//         string.trim().to_string()
//     }
//     pub fn generate_from(&self, text: String) -> String {
//         let text = text.trim().to_lowercase();
//         if text.is_empty() {
//             self.generate()
//         } else {
//             let mut buf = SizedVec::new(self.bufsize);
//             for char in text.chars() {
//                 buf.push(char);
//             }
//             let mut string = String::new();
//             loop {
//                 let blankmap = HashMap::new();
//                 let table = self.table.get(&buf.get_string()).unwrap_or(&blankmap);
//                 let mut total = 0;
//                 for freq in table.values() {
//                     total += freq;
//                 }
//                 if total != 0 {
//                     let mut choice = rand::random::<usize>() % total;
//                     for (char, freq) in table {
//                         if choice < *freq {
//                             string.push_str(char);
//                             buf.push(char.chars().next().unwrap_or('\n'));
//                             break;
//                         } else {
//                             choice -= freq;
//                         }
//                     }
//                 } else {
//                     buf.push('\n')
//                 }
//                 if buf.get_string().ends_with('\n') {
//                     break;
//                 }
//             }
//             string.trim().to_string()
//         }
//     }
// }

// pub struct SizedVec<T> {
//     data: Vec<T>,
//     size: usize,
// }

// impl<T> SizedVec<T> {
//     pub fn new(size: usize) -> SizedVec<T> {
//         SizedVec {
//             data: Vec::new(),
//             size,
//         }
//     }
//     pub fn push(&mut self, item: T) {
//         self.data.push(item);
//         if self.data.len() > self.size {
//             self.data.remove(0);
//         }
//     }
//     pub fn get(&self, index: usize) -> Option<&T> {
//         if index >= self.size {
//             return None;
//         }
//         self.data.get(index)
//     }
// }

// impl SizedVec<char> {
//     pub fn get_string(&self) -> String {
//         let mut string = String::new();
//         for i in 0..self.size {
//             if let Some(c) = self.get(i) {
//                 string.push(*c);
//             }
//         }
//         string
//     }
// }

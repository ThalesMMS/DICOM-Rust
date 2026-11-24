use dicom_rust::dicomjson;
use dicom_rust::net::dicomweb::server::qido::{
    DatasetYield, SearchBackend, SearchLevel, SearchRequest, SearchResult,
};
use dicom_rust::net::dicomweb::server::{
    Backend, SearchMatcher, SearchMatcherOptions, Server, ServerError, ServerOptions,
};
use dicom_rust::net::dicomweb::{
    Client, Endpoint, Options, StudySearchCriteria, study_matches_from_datasets,
    study_search_params,
};
use dicom_rust::net::ul::Ctx;
use dicom_rust::object::{self, Object, Tag, Vr};
use std::error::Error;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
const STUDY_KEYS: &[(u16, u16)] = &[
    (0x0008, 0x0020),
    (0x0008, 0x0030),
    (0x0008, 0x0050),
    (0x0008, 0x1030),
    (0x0010, 0x0010),
    (0x0010, 0x0020),
    (0x0020, 0x000D),
    (0x0020, 0x0010),
];
struct Archive {
    studies: Vec<Object>,
}
fn json_dataset(object: &Object) -> Result<dicomjson::Dataset, ServerError> {
    let body = dicomjson::marshal_compact(object).map_err(|_| ServerError::Backend)?;
    let array = [b"[".as_slice(), &body, b"]"].concat();
    let mut out = None;
    dicomjson::decode_raw_datasets(&array, Default::default(), &mut |dataset| {
        out = Some(dataset);
        Ok(())
    })
    .map_err(|_| ServerError::Backend)?;
    out.ok_or(ServerError::Backend)
}
impl SearchBackend for Archive {
    fn search(
        &self,
        _ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, Result<(), ServerError>) {
        let done = SearchResult {
            remaining: Some(0),
            ..SearchResult::default()
        };
        if request.level != SearchLevel::Study {
            return (done, Ok(()));
        }
        let matcher = match SearchMatcher::new(request, SearchMatcherOptions::default()) {
            Ok(m) => m,
            Err(e) => return (done, Err(e)),
        };
        for study in &self.studies {
            let hit = matcher.matches(Some(study)).and_then(|hit| {
                if hit {
                    json_dataset(study).map(Some)
                } else {
                    Ok(None)
                }
            });
            let sent = match hit {
                Ok(Some(dataset)) => yield_dataset(dataset),
                Ok(None) => Ok(()),
                Err(e) => Err(e),
            };
            if let Err(e) = sent {
                return (done, Err(e));
            }
        }
        (done, Ok(()))
    }
}
impl Backend for Archive {
    fn search_backend(&self) -> Option<&dyn SearchBackend> {
        Some(self)
    }
}
fn archive() -> Result<Archive, Box<dyn Error>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/ct_small.dcm");
    let file = object::open_file(path).map_err(|(_, e)| e)?;
    let mut first = Object::new();
    for &(group, element) in STUDY_KEYS {
        if let Some(e) = file.dataset.get(Tag::new(group, element)) {
            first.put(e.clone());
        }
    }
    let mut second = Object::new();
    for (group, element, vr, value) in [
        (0x0008, 0x0020, Vr::DA, "20230615"),
        (0x0008, 0x1030, Vr::LO, "Chest"),
        (0x0010, 0x0010, Vr::PN, "DOE^JANE"),
        (0x0010, 0x0020, Vr::LO, "PID0002"),
        (0x0020, 0x000D, Vr::UI, "2.25.98765432109876543210987654321"),
    ] {
        second.set_strings(Tag::new(group, element), vr, &[value.to_string()])?;
    }
    Ok(Archive {
        studies: vec![first, second],
    })
}
fn main() -> Result<(), Box<dyn Error>> {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let spool = std::env::temp_dir().join(format!("dicom-rust-web-{}", std::process::id()));
    std::fs::create_dir_all(&spool)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let server = Server::new(ServerOptions {
        backend: Some(Arc::new(archive()?)),
        allow_unauthenticated: true,
        spool_directory: spool.display().to_string(),
        ..ServerOptions::default()
    })?;
    let serving = server.clone();
    let worker = thread::spawn(move || serving.serve(listener));
    let client = Client {
        endpoint: Endpoint {
            base_url: base_url.clone(),
            ..Endpoint::default()
        },
        options: Options::default(),
    };
    let params = study_search_params(&StudySearchCriteria {
        patient_name: filter.clone(),
        ..StudySearchCriteria::default()
    })?;
    let datasets = client.search_studies(&Ctx::background(), &params)?;
    let matches = study_matches_from_datasets(&datasets.unwrap_or_default());
    println!("{base_url}: {} studies match {filter:?}", matches.len());
    for m in &matches {
        println!(
            "  {} {:<14} {:<8} {:<10} {}",
            m.study_date, m.patient_name, m.patient_id, m.study_description, m.study_instance_uid
        );
    }
    server.shutdown(Some(Duration::from_secs(5)))?;
    let _ = worker.join();
    let _ = std::fs::remove_dir_all(&spool);
    Ok(())
}

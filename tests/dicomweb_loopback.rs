#![cfg(feature = "dicomweb")]
#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::dicomjson;
use dicom_rust::net::dicomweb::server::qido::{
    DatasetYield, SearchBackend, SearchLevel, SearchRequest, SearchResult,
};
use dicom_rust::net::dicomweb::server::stow::{
    StoreBackend, StoreOutcome, StoreRequest, StoreStatus,
};
use dicom_rust::net::dicomweb::server::wado::{
    PartReader, RetrieveBackend, RetrievePart, RetrieveRequest, RetrieveYield,
};
use dicom_rust::net::dicomweb::server::{Backend, Server, ServerError, ServerOptions};
use dicom_rust::net::dicomweb::types::{Endpoint, InstanceRef};
use dicom_rust::net::dicomweb::urlrules::Values;
use dicom_rust::net::dicomweb::{Client, Options, RetrieveOptions, StoreInstance};
use dicom_rust::net::ul::Ctx;
use dicom_rust::object::{self, Object, Tag, Vr, writer};
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
struct Stored {
    reference: InstanceRef,
    sop_class: String,
    syntax: String,
    bytes: Vec<u8>,
}
#[derive(Default)]
struct Memory {
    stored: Mutex<Vec<Stored>>,
}
struct Part(Cursor<Vec<u8>>);
impl Read for Part {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}
impl PartReader for Part {}
impl StoreBackend for Memory {
    fn store(&self, _: &Ctx, request: StoreRequest<'_>) -> StoreOutcome {
        let mut bytes = Vec::new();
        if let Err(e) = request.reader.read_to_end(&mut bytes) {
            panic!("store body: {e}");
        }
        self.stored.lock().unwrap().push(Stored {
            reference: request.reference.clone(),
            sop_class: request.sop_class_uid.clone(),
            syntax: request.transfer_syntax_uid.clone(),
            bytes,
        });
        StoreOutcome {
            status: StoreStatus::Stored,
            reference: request.reference,
            sop_class_uid: request.sop_class_uid,
            ..Default::default()
        }
    }
}
fn search_dataset(s: &Stored) -> dicomjson::Dataset {
    let ds = Object::from_elements(vec![
        text(0x0008, 0x0016, Vr::UI, &s.sop_class),
        text(0x0008, 0x0018, Vr::UI, &s.reference.sop_instance_uid),
        text(0x0020, 0x000D, Vr::UI, &s.reference.study_instance_uid),
        text(0x0020, 0x000E, Vr::UI, &s.reference.series_instance_uid),
    ]);
    let json = [
        b"[".as_slice(),
        &dicomjson::marshal_compact(&ds).unwrap(),
        b"]",
    ]
    .concat();
    let mut out = Vec::new();
    dicomjson::decode_raw_datasets(&json, Default::default(), &mut |d| {
        out.push(d);
        Ok(())
    })
    .expect("search data set");
    out.remove(0)
}
impl SearchBackend for Memory {
    fn search(
        &self,
        _: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, Result<(), ServerError>) {
        assert_eq!(request.level, SearchLevel::Instance);
        for s in self.stored.lock().unwrap().iter() {
            let r = &s.reference;
            if (request.study_instance_uid.is_empty()
                || request.study_instance_uid == r.study_instance_uid)
                && (request.series_instance_uid.is_empty()
                    || request.series_instance_uid == r.series_instance_uid)
                && let Err(e) = yield_dataset(search_dataset(s))
            {
                return (SearchResult::default(), Err(e));
            }
        }
        (SearchResult::default(), Ok(()))
    }
}
impl RetrieveBackend for Memory {
    fn retrieve(
        &self,
        _: &Ctx,
        request: &RetrieveRequest,
        yield_part: &mut RetrieveYield<'_>,
    ) -> Result<(), ServerError> {
        let parts: Vec<RetrievePart> = self
            .stored
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.reference.sop_instance_uid == request.sop_instance_uid)
            .map(|s| RetrievePart {
                reference: s.reference.clone(),
                content_type: "application/dicom".into(),
                transfer_syntax_uid: s.syntax.clone(),
                reader: Some(Box::new(Part(Cursor::new(s.bytes.clone())))),
                size: s.bytes.len() as i64,
            })
            .collect();
        if parts.is_empty() {
            return Err(ServerError::status(404, "not-found", ServerError::NotFound));
        }
        for p in parts {
            yield_part(p)?;
        }
        Ok(())
    }
}
impl Backend for Memory {
    fn search_backend(&self) -> Option<&dyn SearchBackend> {
        Some(self)
    }
    fn retrieve_backend(&self) -> Option<&dyn RetrieveBackend> {
        Some(self)
    }
    fn store_backend(&self) -> Option<&dyn StoreBackend> {
        Some(self)
    }
}
const STUDY: &str = "1.2.826.0.1.3680043.10.6.1";
const SERIES: &str = "1.2.826.0.1.3680043.10.6.1.1";
fn instance(n: u32) -> (InstanceRef, Vec<u8>) {
    let mut ds = image(6, 5, 1, 8, 8);
    let sop = format!("{SERIES}.{n}");
    ds.put(text(0x0008, 0x0018, Vr::UI, &sop));
    ds.put(text(0x0020, 0x000D, Vr::UI, STUDY));
    ds.put(text(0x0020, 0x000E, Vr::UI, SERIES));
    let bytes = writer::write_file(&file(ds, EXPLICIT_LE)).unwrap();
    let r = InstanceRef {
        study_instance_uid: STUDY.into(),
        series_instance_uid: SERIES.into(),
        sop_instance_uid: sop,
    };
    (r, bytes)
}
#[test]
fn store_search_and_retrieve() {
    let memory = Arc::new(Memory::default());
    let server = Server::new(ServerOptions {
        backend: Some(memory.clone()),
        allow_unauthenticated: true,
        ..Default::default()
    })
    .expect("server");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let serving = server.clone();
    let handle = thread::spawn(move || serving.serve(listener));
    let client = Client {
        endpoint: Endpoint {
            base_url: format!("http://{addr}"),
            ..Default::default()
        },
        options: Options::default(),
    };
    let ctx = Ctx::background();
    let (refs, files): (Vec<InstanceRef>, Vec<Vec<u8>>) = (1..=3).map(instance).unzip();
    let mut uploads: Vec<StoreInstance> = refs
        .iter()
        .zip(&files)
        .map(|(r, bytes)| StoreInstance {
            sop_class_uid: SECONDARY_CAPTURE.into(),
            sop_instance_uid: r.sop_instance_uid.clone(),
            data: bytes.clone(),
            size: bytes.len() as i64,
            ..Default::default()
        })
        .collect();
    let (result, err) = client.store_instances(&ctx, &mut uploads);
    assert!(err.is_none(), "{err:?}");
    assert_eq!(
        result.stored.as_ref().map(Vec::len),
        Some(3),
        "{:?}",
        result.failed
    );
    assert_eq!(memory.stored.lock().unwrap().len(), 3);
    let found = client
        .search_instances(&ctx, STUDY, SERIES, &Values::new())
        .expect("search")
        .unwrap_or_default();
    assert_eq!(found.len(), 3);
    let mut uids: Vec<String> = found
        .iter()
        .map(|d| dicomjson::element_string(d, "00080018"))
        .collect();
    uids.sort();
    assert_eq!(
        uids,
        refs.iter()
            .map(|r| r.sop_instance_uid.clone())
            .collect::<Vec<_>>()
    );
    let none = client
        .search_instances(&ctx, "1.2.3.4", "1.2.3.4.5", &Values::new())
        .expect("search");
    assert!(none.unwrap_or_default().is_empty());
    for (r, bytes) in refs.iter().zip(&files) {
        let parts = client
            .retrieve_instance(&ctx, r, &RetrieveOptions::default())
            .expect("retrieve");
        assert_eq!(parts.len(), 1);
        assert_eq!(&parts[0].data, bytes, "{}", r.sop_instance_uid);
        let f = object::read_file(&parts[0].data)
            .map_err(|(_, e)| e)
            .unwrap();
        assert_eq!(
            f.dataset.get_string(Tag::new(0x0008, 0x0018)).as_deref(),
            Some(r.sop_instance_uid.as_str())
        );
    }
    let missing = InstanceRef {
        sop_instance_uid: format!("{SERIES}.99"),
        ..refs[0].clone()
    };
    assert!(
        client
            .retrieve_instance(&ctx, &missing, &RetrieveOptions::default())
            .is_err()
    );
    server
        .shutdown(Some(Duration::from_secs(5)))
        .expect("shutdown");
    handle.join().expect("server thread").ok();
}

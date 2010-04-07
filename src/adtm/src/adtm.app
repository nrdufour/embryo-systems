{application, adtm,
    [{description, "Embryo Systems - ADTM"},
     {vsn, "0.1"},
     {modules, [adtm, adtm_sup,
            adtm_class, adtm_attribute, adtm_link, adtm_object,
            adtm_storage_server]},
     {registered, [adtm_class, adtm_attribute, adtm_link, adtm_object,
               adtm_storage_server, adtm_sup]},
     {applications, [kernel,stdlib]},
     {mod, {adtm, []}},
     {start_phases, []}
]}.


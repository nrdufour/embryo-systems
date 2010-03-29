{application, embryosys,
    [{description, "Embryo Systems"},
     {vsn, "0.1"},
     {modules, [embryosys, embryosys_sup,
            embryosys_adtm_class, embryosys_adtm_attribute, embryosys_adtm_link, embryosys_adtm_object,
            embryosys_storage_server]},
     {registered, [embryosys_adtm_class, embryosys_adtm_attribute, embryosys_adtm_link, embryosys_adtm_object,
               embryosys_storage_server, embryosys_sup]},
     {applications, [kernel,stdlib]},
     {mod, {embryosys, []}},
     {start_phases, []}
]}.


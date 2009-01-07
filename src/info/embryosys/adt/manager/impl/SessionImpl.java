/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Repository;
import info.embryosys.adt.manager.Session;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.request.RequestFactory;

import java.util.Queue;

/**
 * @author nrdufour
 * 
 */
public class SessionImpl implements Session {

	private AdtManager adtManager;

	private Workspace workspace;

	private Repository repository;

	private Queue<AdtRequest> requests;

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.Session#commit()
	 */
	@Override
	public void commit() {
		AdtRequest request = null;
		while ((request = requests.poll()) != null) {
			executeRequest(request);
		}
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.Session#doThis(java.lang.String)
	 */
	@Override
	public void doThis(String query) {
		AdtRequest request = RequestFactory.analyzeString(query);
		this.requests.add(request);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.Session#execute(java.lang.String)
	 */
	@Override
	public void execute(String query) {
		AdtRequest request = RequestFactory.analyzeString(query);
		executeRequest(request);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.Session#getRepository()
	 */
	@Override
	public Repository getRepository() {
		return this.repository;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.manager.Session#getWorkspace()
	 */
	@Override
	public Workspace getWorkspace() {
		return this.workspace;
	}

	private void executeRequest(AdtRequest request) {
		try {
			this.adtManager.processRequest(request);
		} catch (Exception e) {
			// FIXME need to do something here and better than just
			// exception
		}
	}
}
